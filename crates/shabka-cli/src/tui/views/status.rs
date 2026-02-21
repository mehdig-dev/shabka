use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::{app::App, widgets::help_bar::HelpBar};

pub fn render(frame: &mut Frame, app: &App, area: Rect, storage_info: &str, provider_info: &str) {
    let layout = Layout::vertical([
        Constraint::Length(9), // info block
        Constraint::Min(5),    // kind breakdown
        Constraint::Length(1), // help bar
    ])
    .split(area);

    // Info block
    let total: usize = app.kind_counts.iter().map(|(_, c)| c).sum();
    let info_lines = vec![
        Line::from(vec![
            Span::styled("  Storage:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(storage_info.to_string(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(provider_info.to_string(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("  Memories: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                total.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Kinds:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.kind_counts.len().to_string(),
                Style::default().fg(Color::Green),
            ),
        ]),
    ];

    let info = Paragraph::new(info_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" System Status "),
    );
    frame.render_widget(info, layout[0]);

    // Kind breakdown as horizontal bar chart
    let max_count = app.kind_counts.iter().map(|(_, c)| *c).max().unwrap_or(1);

    let bar_width = layout[1].width.saturating_sub(25) as usize;

    let bar_lines: Vec<Line> = if app.kind_counts.is_empty() {
        vec![Line::from(Span::styled(
            "  No memories yet.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.kind_counts
            .iter()
            .map(|(kind, count)| {
                let filled = if max_count > 0 {
                    (*count as f64 / max_count as f64 * bar_width as f64) as usize
                } else {
                    0
                }
                .max(1);

                let bar = "█".repeat(filled);
                let empty = "░".repeat(bar_width.saturating_sub(filled));

                Line::from(vec![
                    Span::styled(
                        format!("  {:<12}", kind),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(bar, Style::default().fg(Color::Cyan)),
                    Span::styled(empty, Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" {count}"), Style::default().fg(Color::Green)),
                ])
            })
            .collect()
    };

    let bars = Paragraph::new(bar_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Kind Breakdown "),
    );
    frame.render_widget(bars, layout[1]);

    // Help bar
    frame.render_widget(
        HelpBar {
            screen: &app.screen,
            input_mode: &app.input_mode,
        },
        layout[2],
    );
}
