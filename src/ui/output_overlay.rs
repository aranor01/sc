use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};

fn to_color(c: crate::config::Color) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub struct OutputOverlayWidget<'a> {
    pub cs: &'a ColorScheme,
    pub text: &'a str,
    pub scroll: u16,
}

impl<'a> Widget for OutputOverlayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(Clear, area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .title(Span::from(" Command Output (Esc/C-o to close) "))
            .style(Style::default().bg(to_color(self.cs.panel_bg)));

        let inner = block.inner(area);
        block.render(area, buf);

        let style = Style::default()
            .fg(to_color(self.cs.panel_fg))
            .bg(to_color(self.cs.panel_bg));

        let total_lines = self.text.lines().count();

        let text_area = Rect { width: inner.width.saturating_sub(1), ..inner };

        let para = Paragraph::new(self.text)
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        Widget::render(para, text_area, buf);

        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            width: 1,
            ..inner
        };
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(self.scroll as usize);
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            scrollbar_area,
            buf,
            &mut scrollbar_state,
        );
    }
}
