use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};

/// A text input widget for search, with cursor and focus highlight.
pub struct SearchInput<'a> {
    pub text: &'a str,
    pub cursor: usize,
    pub focused: bool,
}

impl Widget for SearchInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(if self.focused {
                " Search (Enter to submit, Esc to cancel) "
            } else {
                " Search (press /) "
            });

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Render text with cursor
        let prefix = Span::styled("❯ ", Style::default().fg(Color::Cyan));
        let before_cursor = &self.text[..self.cursor.min(self.text.len())];
        let after_cursor = &self.text[self.cursor.min(self.text.len())..];

        let mut spans = vec![prefix, Span::raw(before_cursor)];

        if self.focused {
            // Show cursor character
            let cursor_char = after_cursor.chars().next().unwrap_or(' ');
            spans.push(Span::styled(
                cursor_char.to_string(),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
            if after_cursor.len() > cursor_char.len_utf8() {
                spans.push(Span::raw(&after_cursor[cursor_char.len_utf8()..]));
            }
        } else {
            spans.push(Span::raw(after_cursor));
        }

        let line = Line::from(spans);
        buf.set_line(inner.x, inner.y, &line, inner.width);
    }
}
