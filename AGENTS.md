# AGENTS.md

This file provides guidance to coding agents when working with code in this repository.

## What this is

A Windows-only Rust desktop widget: a transparent, always-on-top, click-through overlay window drawn with raw GDI, optionally playing a video (decoded via ffmpeg) as its background. See `README.md` for the user-facing feature list (transparent/semitransparent window, click-through, draggable, custom GDI drawing, hotkeys).

## Commands

- Build: `cargo build`
- Run without a video (animated gradient background): `cargo run`
- Run with a video background: `cargo run -- path/to/video.mp4` (two sample files, `sample-3.mp4` and `sample-5.mp4`, exist in the repo root for manual testing but are gitignored)
- Lint (must pass with zero warnings, matches CI/pre-commit): `cargo clippy --all --color=always -- -D warnings`
- Format: `cargo fmt --all`
- Type/borrow check only: `cargo check --all`
- Tests: `cargo test` (unit tests currently live in `src/state.rs`, e.g. `cargo test hit_test_passthrough_inside_circle` for a single test)
- Logging: the app uses `tracing`/`tracing-subscriber`; run with `RUST_LOG=debug` or `RUST_LOG=trace` (e.g. `RUST_LOG=debug cargo run -- sample-5.mp4`) to see decoder/window lifecycle logs.

Pre-commit hooks (via `prek`, configured in `.pre-commit-config.yaml`) run `cargo fmt --all`, `cargo check --all`, and `cargo clippy --all -- -D warnings` on commit, plus standard hygiene checks (trailing whitespace, large files, private keys, etc.). Run these manually before committing if hooks aren't installed locally.

`Cargo.toml` enables `clippy::pedantic` as a warning group, with several cast-related pedantic lints explicitly allowed (`cast_possible_truncation`, `cast_sign_loss`, `cast_precision_loss`, `cast_possible_wrap`) since GDI/Win32 interop requires a lot of numeric casting.

## Architecture

The app is split across a fixed-purpose module set; the interesting behavior comes from how they communicate across threads, not from any one file in isolation.

- **`main.rs`** — parses an optional video file path argument (`clap`). If given, it creates two channels and a shared FPS handle, then spawns a background thread running `FrameStream::read_frames`:
  - `size_sync`: unbounded `mpsc::channel<PhysicalSize<u32>>`, window → decoder, notifies the decoder of resizes.
  - `frame_sync`: **bounded** `mpsc::sync_channel::<frame::Video>(2)`, decoder → window.
  - `fps`: `Arc<AtomicU64>` storing an `f64` (as `to_bits()`/`from_bits()`), decoder → window, communicates the real video frame rate.
  - Without a file, `App::new()` just runs the animated-gradient-only mode.

- **`ff.rs` (`FrameStream`)** — runs entirely on the background decode thread. Opens the file with `ffmpeg-next`, prefers the `h264_cuvid` hardware decoder and falls back to software, and scales every decoded frame to `BGRA` via `ffmpeg_next::software::scaling::Context` before sending it to the window. Two things here are load-bearing and easy to regress:
  - The frame channel **must stay bounded**. `frame_sync.send()` blocking is what paces decoding to real playback speed — with an unbounded channel the decoder races ahead, dumps the whole video into the channel in a couple of seconds, and then exits long before the user can resize the window (the resize channel then has no reader left).
  - When a new size arrives on `size_sync`, the scaler is rebuilt with the new output dimensions, and the reused output frame buffer (`frame_buffer_after`) **must be reset to `frame::Video::empty()`**. `Scaler::run()` only reallocates its output frame when it's empty; otherwise a stale-sized buffer causes `Error::OutputChanged` and kills the decode thread.

- **`window.rs` (`App`)** — implements winit's `ApplicationHandler`. Owns the actual `Window` (transparent, undecorated, `AlwaysOnTop`, `skip_taskbar`) and does all rendering by hand in `draw_gdi`: pulls the newest available frame off `frame_sync` (non-blocking `try_recv`, so it just keeps the last frame if none is ready), copies its `BGRA` bytes into a DIB section (falling back to an animated diagonal gradient if no video/frame is available), sets per-pixel alpha for transparency, and draws a pulsing black circle at the window center via GDI `Ellipse`. Redraw cadence in `about_to_wait` is paced by `frame_interval()`, which reads the shared `fps` `AtomicU64` (defaults to ~60fps if unset).
  - Handles keyboard: `Escape` quits, `W/A/S/D` moves the window, `F` toggles fullscreen, `+`/`-` adjust transparency.
  - On window creation it sets `WS_EX::LAYERED` and `SetLayeredWindowAttributes` with a color-key so pixels matching `colors::BLACK` become click-through, and installs `state::custom_wndproc` as the window procedure.

- **`state.rs`** — a mutex-guarded `GLOBAL_STATE` singleton (phase of the pulsing-circle animation, fullscreen toggle state, saved pre-fullscreen position/size, and the original `WNDPROC` pointer) plus `custom_wndproc`, the replacement window procedure. It intercepts `WM_NCHITTEST` to make the window draggable everywhere except inside the pulsing circle (hit-tested against the same phase-driven radius used for drawing, so the click-through hole tracks the animation), and intercepts double-click/`SC_MAXIMIZE` to call `toggle_fullscreen` instead of native maximize. Unhandled messages are forwarded to the saved original proc — the mutex is always dropped before calling into Win32 to avoid deadlocking on reentrant messages.

- **`colors.rs`** — shared color constants (e.g. the `BLACK` color-key used for click-through transparency).

- **`error.rs`** — a single `thiserror`-based `Error`/`Result` used everywhere, wrapping `ffmpeg_next::Error`, `winsafe::co::ERROR`, `winit::error::EventLoopError`, and `mpsc::SendError<frame::Video>`.

### Threading model at a glance

``` text
main thread (winit event loop, App)  <--- frame_sync (bounded, 2) --- decode thread (FrameStream)
                                       --- size_sync (unbounded)   --->
                                      <--- fps (Arc<AtomicU64>)    ---
```

The decode thread is the only writer of decoded frames and the only reader of resize events; the main thread is the only writer of resize events and reads frames opportunistically each redraw. Keeping `frame_sync` bounded is what keeps the decode thread alive and responsive for the whole video instead of finishing (and exiting) far ahead of real-time playback.
