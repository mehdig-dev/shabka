use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

const LOGO: &[&str] = &[
    r"     _           _     _         ",
    r"    | |         | |   | |        ",
    r" ___| |__   __ _| |__ | | ____ _ ",
    r"/ __| '_ \ / _` | '_ \| |/ / _` |",
    r"\__ \ | | | (_| | |_) |   < (_| |",
    r"|___/_| |_|\__,_|_.__/|_|\_\__,_|",
];

pub fn render(
    frame: &mut Frame,
    area: Rect,
    storage: &str,
    provider: &str,
    memory_count: usize,
    loading: bool,
) {
    let block_height = LOGO.len() as u16 + 8;
    let block_width = 50;

    let [center_y] = Layout::vertical([Constraint::Length(block_height)])
        .flex(Flex::Center)
        .areas(area);
    let [center] = Layout::horizontal([Constraint::Length(block_width)])
        .flex(Flex::Center)
        .areas(center_y);

    let mut lines: Vec<Line> = Vec::new();

    // Logo
    for row in LOGO {
        lines.push(Line::from(Span::styled(
            *row,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));

    // Tagline
    lines.push(Line::from(Span::styled(
        "        Shared LLM Memory System",
        Style::default().fg(Color::DarkGray),
    )));

    lines.push(Line::from(""));

    // Info
    lines.push(Line::from(vec![
        Span::styled("    storage ", Style::default().fg(Color::DarkGray)),
        Span::styled(storage, Style::default().fg(Color::Magenta)),
        Span::styled("  ·  provider ", Style::default().fg(Color::DarkGray)),
        Span::styled(provider, Style::default().fg(Color::Magenta)),
    ]));

    lines.push(Line::from(""));

    // Status line — loading or ready
    if loading {
        lines.push(Line::from(Span::styled(
            "          Loading memories...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("             ", Style::default()),
            Span::styled(
                format!("{memory_count}"),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if memory_count == 1 {
                    " memory loaded"
                } else {
                    " memories loaded"
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), center);
}
