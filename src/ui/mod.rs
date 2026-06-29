pub mod button;
pub mod button_bar;
pub mod cmdline;
pub mod dialog;
pub mod modal_event;
pub mod popup_list;
pub mod menu;
pub mod output_overlay;
pub mod panel;
pub mod status_bar;

pub fn to_color(c: crate::config::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(c.0, c.1, c.2)
}
