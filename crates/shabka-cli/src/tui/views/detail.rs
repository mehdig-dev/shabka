use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use shabka_core::model::{RelationType, VerificationStatus};

use crate::tui::{app::App, widgets::help_bar::HelpBar};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ref memory) = app.detail_memory else {
        let msg = Paragraph::new("No memory loaded.").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    };

    let layout = Layout::vertical([
        Constraint::Length(2), // title
        Constraint::Length(1), // meta line
        Constraint::Min(5),    // content (scrollable)
        Constraint::Length(1), // help bar
    ])
    .split(area);

    // Title
    let title_text = Line::from(vec![
        Span::styled(
            format!(" {} ", memory.kind),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&memory.title, Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let title_widget = Paragraph::new(title_text).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(title_widget, layout[0]);

    // Meta line: ID | created | importance | trust | verification
    let (ver_text, ver_color) = match memory.verification {
        VerificationStatus::Verified => ("✓ verified", Color::Green),
        VerificationStatus::Disputed => ("✗ disputed", Color::Red),
        VerificationStatus::Outdated => ("⚠ outdated", Color::Yellow),
        VerificationStatus::Unverified => ("unverified", Color::DarkGray),
    };

    let trust_color = if app.detail_trust >= 0.7 {
        Color::Green
    } else if app.detail_trust >= 0.4 {
        Color::Yellow
    } else {
        Color::Red
    };

    let meta = Line::from(vec![
        Span::styled(
            format!(" {} ", &memory.id.to_string()[..8]),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            memory.created_at.format("%Y-%m-%d %H:%M").to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(" │ imp: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.0}%", memory.importance * 100.0),
            Style::default().fg(if memory.importance >= 0.7 {
                Color::Green
            } else if memory.importance >= 0.4 {
                Color::Yellow
            } else {
                Color::Red
            }),
        ),
        Span::styled(" │ trust: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.0}%", app.detail_trust * 100.0),
            Style::default().fg(trust_color),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(ver_text, Style::default().fg(ver_color)),
        Span::styled(
            format!(
                " │ tags: {}",
                if memory.tags.is_empty() {
                    "—".to_string()
                } else {
                    memory.tags.join(", ")
                }
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(meta), layout[1]);

    // Content area: content + relations + history (all scrollable)
    render_content(frame, app, memory, layout[2]);

    // Help bar
    frame.render_widget(
        HelpBar {
            screen: &app.screen,
            input_mode: &app.input_mode,
        },
        layout[3],
    );
}

fn render_content(frame: &mut Frame, app: &App, memory: &shabka_core::model::Memory, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Content section
    lines.push(Line::from(Span::styled(
        "─── Content ───",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for line in memory.content.lines() {
        lines.push(Line::from(line.to_string()));
    }

    // Relations section
    if !app.detail_relations.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("─── Relations ({}) ───", app.detail_relations.len()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for rel in &app.detail_relations {
            let (arrow, color) = relation_style(&rel.relation_type);
            let target = if rel.source_id == memory.id {
                &rel.target_id
            } else {
                &rel.source_id
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {arrow} "), Style::default().fg(color)),
                Span::styled(
                    rel.relation_type.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" → {}", &target.to_string()[..8]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" (str: {:.0}%)", rel.strength * 100.0),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Relations ───",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  No relations. Use the MCP or CLI to relate memories.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // History section
    if !app.detail_history.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("─── History ({}) ───", app.detail_history.len()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for event in &app.detail_history {
            lines.push(Line::from(Span::styled(
                format!("  • {event}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // Details section
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "─── Details ───",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!("  Source: {}", memory.source)));
    lines.push(Line::from(format!("  Scope: {}", memory.scope)));
    lines.push(Line::from(format!("  Privacy: {}", memory.privacy)));
    lines.push(Line::from(format!("  Status: {}", memory.status)));
    if let Some(ref proj) = memory.project_id {
        lines.push(Line::from(format!("  Project: {proj}")));
    }
    lines.push(Line::from(format!(
        "  Updated: {}",
        memory.updated_at.format("%Y-%m-%d %H:%M")
    )));
    lines.push(Line::from(format!(
        "  Accessed: {}",
        memory.accessed_at.format("%Y-%m-%d %H:%M")
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Detail (j/k to scroll) "),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn relation_style(rel_type: &RelationType) -> (&str, Color) {
    match rel_type {
        RelationType::Fixes => ("🔧", Color::Green),
        RelationType::CausedBy => ("⚡", Color::Red),
        RelationType::Supersedes => ("↑", Color::Yellow),
        RelationType::Contradicts => ("✗", Color::Magenta),
        RelationType::Related => ("→", Color::Blue),
    }
}
