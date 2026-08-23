# Widget

This is a simple interactive widget built using the Windows API in Rust.

## Features

- Transparent window, with semitransparent elements
- Click-through behavior - does not block mouse events to underlying windows where fully transparent
- Movable by dragging
- Custom drawing using GDI
- Simple mouse interaction
- Hotkey support
- **Safe Rust implementation** using the [winsafe](https://github.com/rodrigocfd/winsafe) crate

## Implementation

This project uses `winsafe` instead of raw Windows API bindings for improved safety:
- Type-safe Windows API wrappers
- RAII-based resource management (automatic cleanup)
- Reduced unsafe code blocks
- Idiomatic Rust error handling
