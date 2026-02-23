use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::tui::app::{InputMode, Screen};

/// Bottom help bar showing context-sensitive key bindings.
pub struct HelpBar<'a> {
    pub screen: &'a Screen,
    pub input_mode: &'a InputMode,
}

impl Widget for HelpBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().fg(Color::DarkGray);
        let key_style = Style::default().fg(Color::Cyan);

        let spans: Vec<Span> = match (self.screen, self.input_mode) {
            (Screen::List, InputMode::Normal) => vec![
                Span::styled("j/k", key_style),
                Span::styled(" navigate  ", style),
                Span::styled("Enter", key_style),
                Span::styled(" open  ", style),
                Span::styled("/", key_style),
                Span::styled(" search  ", style),
                Span::styled("f", key_style),
                Span::styled(" filter  ", style),
                Span::styled("n", key_style),
                Span::styled(" new  ", style),
                Span::styled("Tab", key_style),
                Span::styled(" status  ", style),
                Span::styled("r", key_style),
                Span::styled(" refresh  ", style),
                Span::styled("q", key_style),
                Span::styled(" quit", style),
            ],
            (Screen::List, InputMode::Search) => vec![
                Span::styled("Enter", key_style),
                Span::styled(" search  ", style),
                Span::styled("Esc", key_style),
                Span::styled(" cancel", style),
            ],
            (Screen::List, InputMode::Filter) => vec![
                Span::styled("←/→", key_style),
                Span::styled(" cycle kind  ", style),
                Span::styled("Enter/Esc", key_style),
                Span::styled(" confirm", style),
            ],
            (Screen::Detail, _) => vec![
                Span::styled("j/k", key_style),
                Span::styled(" scroll  ", style),
                Span::styled("PgUp/PgDn", key_style),
                Span::styled(" page  ", style),
                Span::styled("e", key_style),
                Span::styled(" edit  ", style),
                Span::styled("Esc", key_style),
                Span::styled(" back  ", style),
                Span::styled("q", key_style),
                Span::styled(" quit", style),
            ],
            (Screen::Create, _) => vec![
                Span::styled("Tab", key_style),
                Span::styled(" next field  ", style),
                Span::styled("Shift+Tab", key_style),
                Span::styled(" prev field  ", style),
                Span::styled("Ctrl+S", key_style),
                Span::styled(" save  ", style),
                Span::styled("Esc", key_style),
                Span::styled(" cancel", style),
            ],
            (Screen::Status, _) => vec![
                Span::styled("Tab/Esc", key_style),
                Span::styled(" back to list  ", style),
                Span::styled("q", key_style),
                Span::styled(" quit", style),
            ],
        };

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
