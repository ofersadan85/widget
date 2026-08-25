# Widget

Windows-only transparent desktop widget written in Rust using raw GDI for drawing and optional ffmpeg-backed video playback.

## Features

- Transparent always-on-top overlay window with adjustable opacity
- Click-through behavior for color-keyed transparent pixels
- Draggable window with pulsing center interaction circle
- Custom software rendering with GDI + BGRA frame blitting
- Optional video background playback (`cargo run -- path/to/video.mp4`)
- On-screen display (top-left): file name and `current / -remaining` playback time
- Playback controls: pause/resume and 5-second seek jumps

## Controls

- `Escape`: quit
- `W` / `A` / `S` / `D`: move window
- `F`: toggle fullscreen
- `+` / `-`: increase/decrease transparency
- `H`: show/hide overlay text
- `Space`: pause/resume video playback
- `Left` / `Right`: seek backward/forward 5 seconds

## Notes

- In gradient-only mode (no video file), the overlay text is hidden automatically.
- During pause, the video frame freezes while the UI redraw loop keeps running.
