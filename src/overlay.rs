use crate::{colors::WHITE, error::Result};
use std::fmt::{self, Write};
use winsafe::{
    HFONT, SIZE, co,
    guard::{DeleteDCGuard, DeleteObjectGuard},
};

/// Represents the overlay text that is drawn on the window
pub struct OverlayText {
    /// The buffer used to store the formatted text
    buffer: String,
    /// Whether to show the overlay text or not
    pub show: bool,
    /// The font used for the overlay text
    font: DeleteObjectGuard<HFONT>,
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
}
