use crate::{
    colors::{BLACK, WHITE},
    error::Result,
};
use std::fmt::{self, Write};
use winit::dpi::PhysicalPosition;
use winsafe::{
    HFONT, SIZE, co,
    guard::{DeleteDCGuard, DeleteObjectGuard},
};

/// Represents the overlay text that is drawn on the window
pub struct OverlayText {
    /// The buffer used to store the formatted text
    buffer: String,
    /// The font used for the overlay text
    font: DeleteObjectGuard<HFONT>,
    /// Whether to show the overlay text or not
    pub show: bool,
}

impl OverlayText {
    pub fn new() -> Result<Self> {
        let font = HFONT::CreateFont(
            SIZE::with(0, 26),
            0,
            0,
            co::FW::NORMAL,
            false,
            false,
            false,
            co::CHARSET::ANSI,
            co::OUT_PRECIS::DEFAULT,
            co::CLIP::DEFAULT_PRECIS,
            co::QUALITY::DEFAULT,
            co::PITCH::DEFAULT,
            "Segoe UI",
        )?;
        Ok(Self {
            buffer: String::new(),
            show: true,
            font,
        })
    }

    fn format(&mut self, file_name: &str, current_pts: f64, duration_secs: f64) -> fmt::Result {
        self.buffer.clear();
        let remaining = (duration_secs - current_pts).max(0.0);
        writeln!(self.buffer, "{file_name}")?;
        format_time(current_pts, &mut self.buffer)?;
        write!(self.buffer, " / -")?;
        format_time(remaining, &mut self.buffer)
    }

    pub fn draw(
        &mut self,
        hdc_mem: &DeleteDCGuard,
        file_name: &str,
        current_pts: f64,
        duration_secs: f64,
    ) -> Result<()> {
        if file_name.is_empty() || !self.show {
            return Ok(());
        }
        let _font_guard = hdc_mem.SelectObject(&*self.font)?;
        self.format(file_name, current_pts, duration_secs)?;
        hdc_mem.SetTextColor(WHITE)?;
        hdc_mem.SetBkMode(co::BKMODE::TRANSPARENT)?;
        for (i, line) in self.buffer.lines().enumerate() {
            hdc_mem.TextOut(8, 22 * (i as i32 + 1) + 8, line)?;
        }
        Ok(())
    }
}

fn format_time(seconds: f64, buffer: &mut String) -> fmt::Result {
    let total_seconds = seconds.max(0.0).round() as i64;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let secs = total_seconds % 60;
    if hours > 0 {
        write!(buffer, "{hours:02}:{minutes:02}:{secs:02}")?;
    } else {
        write!(buffer, "{minutes:02}:{secs:02}")?;
    }
    Ok(())
}

#[derive(Default)]
pub struct PulsingCircle {
    pub hover: bool,
    pub phase: f32,
}

impl PulsingCircle {
    pub fn draw(
        &self,
        hdc_mem: &winsafe::guard::DeleteDCGuard,
        center: PhysicalPosition<f64>,
    ) -> winsafe::SysResult<()> {
        let brush = winsafe::HBRUSH::CreateSolidBrush(BLACK)?;
        let _brush_guard = hdc_mem.SelectObject(&*brush)?;
        let radius = (60.0 + (self.phase.sin() * 30.0)) as i32;
        let ellipse_rect = winsafe::RECT {
            left: center.x as i32 - radius,
            top: center.y as i32 - radius,
            right: center.x as i32 + radius,
            bottom: center.y as i32 + radius,
        };
        hdc_mem.Ellipse(ellipse_rect)
    }
}

pub fn is_cursor_in_circle(
    center: PhysicalPosition<f64>,
    phase: f32,
    cursor_pos: PhysicalPosition<f64>,
) -> bool {
    let dx = cursor_pos.x - center.x;
    let dy = cursor_pos.y - center.y;
    let radius = 60.0 + (f64::from(phase.sin()) * 30.0);
    dx * dx + dy * dy < radius * radius
}

pub fn hit_test_circle(
    center: PhysicalPosition<f64>,
    phase: f32,
    cursor_pos: PhysicalPosition<f64>,
) -> u16 {
    if is_cursor_in_circle(center, phase, cursor_pos) {
        co::HT::TRANSPARENT.raw()
    } else {
        co::HT::CAPTION.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_text() {
        let mut overlay_text = OverlayText::new().unwrap();
        overlay_text.format("test.mp4", 65.0, 3_900.0).unwrap();
        assert_eq!(overlay_text.buffer, "test.mp4\n01:05 / -01:03:55");
    }

    #[test]
    fn mm_ss_and_hh_mm_ss() {
        let mut buffer = String::new();
        format_time(0.0, &mut buffer).unwrap();
        assert_eq!(buffer, "00:00");
        buffer.clear();
        format_time(65.0, &mut buffer).unwrap();
        assert_eq!(buffer, "01:05");
        buffer.clear();
        format_time(3_600.0, &mut buffer).unwrap();
        assert_eq!(buffer, "01:00:00");
    }

    #[test]
    fn hit_test_passthrough_inside_circle() {
        let center = PhysicalPosition::new(200.0, 150.0);
        let cursor = PhysicalPosition::new(220.0, 150.0);
        assert_eq!(
            hit_test_circle(center, 0.0, cursor),
            co::HT::TRANSPARENT.raw()
        );
    }

    #[test]
    fn hit_test_draggable_outside_circle() {
        let center = PhysicalPosition::new(200.0, 150.0);
        let cursor = PhysicalPosition::new(320.0, 150.0);
        assert_eq!(hit_test_circle(center, 0.0, cursor), co::HT::CAPTION.raw());
    }
}
