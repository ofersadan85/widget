# AGENTS.md

This file provides guidance to coding agents when working with code in this repository.

## What this is

A Windows-only Rust desktop widget: a transparent, always-on-top, click-through overlay window drawn with raw GDI, optionally playing a video (decoded via ffmpeg) as its background. The player now includes an on-screen display (file name + `current / -remaining` time), pause/resume, and 5-second seek controls.

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

- **`main.rs`** — parses an optional video file path argument (`clap`). If given, it constructs `App::new_with_stream(file)` and runs the app with decode-thread channels and shared playback state set up by `window.rs`. Without a file, `App::new()` runs animated-gradient-only mode.

- **`ff.rs` (`FrameStream`)** — runs entirely on the background decode thread. Opens the file with `ffmpeg-next`, prefers `h264_cuvid` when available, falls back to software decode, and scales frames to `BGRA` before sending to the window.
  - `PlaybackCommand` (`TogglePause`, `Seek(f64)`) arrives from the window thread over `command_sync`.
  - Frames are sent as `DecodedFrame { frame, pts_seconds }` so the window receives pixel data and timestamp atomically.
  - Stream duration is computed from `input.duration()` and published once via shared `duration` (`Arc<AtomicU64>` storing `f64` bits).
  - Packet consumption is one-packet-per-iteration (`input.packets().next()`) so `input.seek()` can be called safely between reads.
  - On EOF, decode does not exit immediately; it waits for commands (notably seek), allowing rewind after the end.
  - The frame channel **must stay bounded** (`sync_channel(2)`) so decoder throughput is back-pressured to rendering cadence.
  - On resize, scaler output dimensions are rebuilt to match the new window size.

- **`window.rs` (`App`)** — implements winit's `ApplicationHandler` and owns the transparent undecorated `AlwaysOnTop` window. In `draw_gdi`, it:
  - pulls the latest `DecodedFrame` (non-blocking `try_recv`) and keeps the last good frame,
  - writes frame data (or fallback gradient) into a 32-bit bitmap,
  - applies global alpha for semitransparency,
  - draws the pulsing black center circle,
  - draws playback overlay text via `overlay.rs`,
  - blits to screen with `BitBlt`.
  Redraw cadence in `about_to_wait` is paced by `frame_interval()` using shared FPS.
  - Handles keyboard: `Escape` quit, `W/A/S/D` move, `F` fullscreen, `+`/`-` transparency, `H` overlay toggle, `Space` pause/resume, `Left`/`Right` seek ±5s.
  - On seek input, pending frame queue entries are drained before issuing the seek command to avoid stale-frame jumps.
  - On window creation it sets `WS_EX::LAYERED` and `SetLayeredWindowAttributes` with `colors::BLACK` as color-key, then installs `state::custom_wndproc`.

- **`overlay.rs`** — encapsulates OSD state and drawing (`OverlayText`):
  - owns reusable text buffer and selected font,
  - formats two lines (file name, `current / -remaining`),
  - draws top-left with transparent background mode and white text,
  - exposes `show` toggle (`H` key).

- **`state.rs`** — a mutex-guarded `GLOBAL_STATE` singleton (phase of the pulsing-circle animation, fullscreen toggle state, saved pre-fullscreen position/size, and the original `WNDPROC` pointer) plus `custom_wndproc`, the replacement window procedure. It intercepts `WM_NCHITTEST` to make the window draggable everywhere except inside the pulsing circle (hit-tested against the same phase-driven radius used for drawing, so the click-through hole tracks the animation), and intercepts double-click/`SC_MAXIMIZE` to call `toggle_fullscreen` instead of native maximize. Unhandled messages are forwarded to the saved original proc — the mutex is always dropped before calling into Win32 to avoid deadlocking on reentrant messages.

- **`colors.rs`** — shared color constants (e.g. the `BLACK` color-key used for click-through transparency).

- **`error.rs`** — a single `thiserror`-based `Error`/`Result` used everywhere, wrapping `ffmpeg_next::Error`, `winsafe::co::ERROR`, `winit::error::EventLoopError`, and `mpsc::SendError<DecodedFrame>`.

### Threading model at a glance

``` text
main thread (winit event loop, App)  <--- frame_sync (bounded, 2): DecodedFrame --- decode thread (FrameStream)
                                       --- size_sync (unbounded)              --->
                                       --- command_sync (unbounded)           --->
                                      <--- fps (Arc<AtomicU64>)               ---
                                      <--- duration (Arc<AtomicU64>)          ---
```

The decode thread is the only writer of decoded frames and the only reader of resize + playback commands; the main thread is the only writer of resize + playback commands and reads frames opportunistically each redraw. Keeping `frame_sync` bounded is what keeps decode timing tied to the displayed playback instead of racing ahead.
