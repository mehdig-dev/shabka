use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::tui::app::ALL_KINDS;

/// Filter bar showing the current kind filter with cycling indicator.
pub struct FilterBar {
    pub selected_index: usize,
    pub active: bool,
}

impl Widget for FilterBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut spans: Vec<Span> = Vec::new();
        let prefix = if self.active { "Filter: " } else { "Kind: " };
        spans.push(Span::styled(prefix, Style::default().fg(Color::DarkGray)));

        for (i, kind) in ALL_KINDS.iter().enumerate() {
            let label = match kind {
                None => "All",
                Some(k) => match k {
                    shabka_core::model::MemoryKind::Observation => "Obs",
                    shabka_core::model::MemoryKind::Decision => "Dec",
                    shabka_core::model::MemoryKind::Pattern => "Pat",
                    shabka_core::model::MemoryKind::Error => "Err",
                    shabka_core::model::MemoryKind::Fix => "Fix",
                    shabka_core::model::MemoryKind::Preference => "Pref",
                    shabka_core::model::MemoryKind::Fact => "Fact",
                    shabka_core::model::MemoryKind::Lesson => "Les",
                    shabka_core::model::MemoryKind::Todo => "Todo",
                    shabka_core::model::MemoryKind::Procedure => "Proc",
                },
            };

            let style = if i == self.selected_index {
                if self.active {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                }
            } else {
                Style::default().fg(Color::DarkGray)
            };

            spans.push(Span::styled(format!(" {label} "), style));

            if i < ALL_KINDS.len() - 1 {
                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            }
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
