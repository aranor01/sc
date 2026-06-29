use std::time::{Duration, Instant};

use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::config::ColorScheme;
use super::to_color;

// ── StatusMsg ─────────────────────────────────────────────────────────────────

struct StatusMsg {
    text: String,
    expiry: Instant,
    warn: bool,
}

// ── StatusBarState ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct StatusBarState {
    msg: Option<StatusMsg>,
}

impl StatusBarState {
    pub fn set(&mut self, text: &str, warn: bool) {
        self.msg = Some(StatusMsg {
            text: text.to_string(),
            expiry: Instant::now() + Duration::from_secs(3),
            warn,
        });
    }

    pub fn is_active(&self) -> bool {
        self.msg.as_ref().map(|m| Instant::now() < m.expiry).unwrap_or(false)
    }

    pub fn expire(&mut self) {
        if self.msg.as_ref().map(|m| Instant::now() >= m.expiry).unwrap_or(false) {
            self.msg = None;
        }
    }
}

// ── StatusBarWidget ───────────────────────────────────────────────────────────

pub struct StatusBarWidget<'a> {
    pub cs: &'a ColorScheme,
    pub state: &'a StatusBarState,
}

impl<'a> Widget for StatusBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let Some(ref msg) = self.state.msg else { return };

        let (fg, bg) = if msg.warn {
            (to_color(self.cs.status_warn_fg), to_color(self.cs.status_warn_bg))
        } else {
            (to_color(self.cs.status_info_fg), to_color(self.cs.status_info_bg))
        };

        let style = Style::default().fg(fg).bg(bg);
        let text = format!(" {} ", msg.text);

        // Fill the row with the background colour first, then overlay the text.
        let fill = " ".repeat(area.width as usize);
        buf.set_string(area.x, area.y, &fill, style);
        buf.set_string(area.x, area.y, &text, style);
    }
}
