pub mod button;
pub mod button_bar;
pub mod cmdline;
pub mod dialog;
pub mod focus;
pub mod modal_event;
pub mod popup_list;
pub mod menu;
pub mod output_overlay;
pub mod panel;
pub mod status_bar;

use unicode_width::UnicodeWidthChar;

pub fn to_color(c: crate::config::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(c.0, c.1, c.2)
}

/// Expands tab characters to spaces at 8-column tab stops. A raw tab written
/// straight to the terminal makes the physical cursor jump columns that
/// ratatui's buffer model doesn't know about, desyncing everything drawn
/// after it on that row — including the panel/overlay border, which ends up
/// overwritten or skipped. Column tracking uses Unicode display width, like
/// the rest of the rendering pipeline.
pub fn expand_tabs(line: &str) -> String {
    const TAB_WIDTH: usize = 8;
    let mut out = String::with_capacity(line.len());
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = TAB_WIDTH - (col % TAB_WIDTH);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            col += ch.width().unwrap_or(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tabs_at_start_of_line_uses_full_tab_stop() {
        assert_eq!(expand_tabs("\tx"), "        x");
    }

    #[test]
    fn expand_tabs_mid_line_advances_to_next_stop() {
        assert_eq!(expand_tabs("abc\tx"), "abc     x");
    }

    #[test]
    fn expand_tabs_exactly_on_a_stop_still_advances_a_full_stop() {
        assert_eq!(expand_tabs("12345678\tx"), "12345678        x");
    }

    #[test]
    fn expand_tabs_multiple_tabs_accumulate_correctly() {
        assert_eq!(expand_tabs("a\tb\tc"), "a       b       c");
    }

    #[test]
    fn expand_tabs_no_tabs_is_unchanged() {
        assert_eq!(expand_tabs("no tabs here"), "no tabs here");
    }
}
