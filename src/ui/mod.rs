pub mod button_bar;
pub mod cmdline;
pub mod dialog;
pub mod menu;
pub mod output_overlay;
pub mod panel;

pub fn to_color(c: crate::config::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(c.0, c.1, c.2)
}
