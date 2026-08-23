#![allow(dead_code)]

use winsafe::COLORREF;

pub const BLACK: COLORREF = COLORREF::from_rgb(0, 0, 0);
pub const WHITE: COLORREF = COLORREF::from_rgb(255, 255, 255);
pub const RED: COLORREF = COLORREF::from_rgb(255, 0, 0);
pub const GREEN: COLORREF = COLORREF::from_rgb(0, 255, 0);
pub const BLUE: COLORREF = COLORREF::from_rgb(0, 0, 255);
pub const YELLOW: COLORREF = COLORREF::from_rgb(255, 255, 0);
pub const CYAN: COLORREF = COLORREF::from_rgb(0, 255, 255);
pub const MAGENTA: COLORREF = COLORREF::from_rgb(255, 0, 255);
