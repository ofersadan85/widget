use winsafe::{
    co::{LWA, SW},
    DispatchMessage, GetMessage, SysResult, MSG,
};

mod api;
pub use api::*;

fn main() -> SysResult<()> {
    let hwnd = create_window(
        Some("Transparent Widget Test"),
        WINDOW_POSITION,
        WINDOW_SIZE,
    )?;
    hwnd.ShowWindow(SW::SHOW);
    // Set initial layered window attributes
    hwnd.SetLayeredWindowAttributes(ALPHA_KEY, 255, LWA::COLORKEY | LWA::ALPHA)?;
    // 60 FPS timer
    hwnd.SetTimer(1, 16, None)?;

    let mut msg = MSG::default();
    while GetMessage(&mut msg, None, 0, 0)? {
        // let _ = TranslateMessage(&msg);
        unsafe { DispatchMessage(&msg) };
    }
    Ok(())
}
