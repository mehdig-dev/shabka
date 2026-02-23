use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::{
    app::{App, CREATE_KINDS},
    widgets::help_bar::HelpBar,
};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([
        Constraint::Length(3), // Title input
        Constraint::Min(5),    // Content input
        Constraint::Length(3), // Kind selector
        Constraint::Length(1), // Help bar
    ])
    .split(area);

    // Title field
    let title_border_style = if app.create_field == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(title_border_style)
        .title(" Title ");
    let title_text = Paragraph::new(app.create_title.as_str()).block(title_block);
    frame.render_widget(title_text, layout[0]);

    // Show cursor in focused text field
    if app.create_field == 0 {
        let cursor_x = layout[0].x + 1 + app.create_title.len() as u16;
        let cursor_y = layout[0].y + 1;
        frame.set_cursor_position((cursor_x.min(layout[0].right() - 2), cursor_y));
    }

    // Content field
    let content_border_style = if app.create_field == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style)
        .title(" Content ");
    let content_text = Paragraph::new(app.create_content.as_str())
        .block(content_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(content_text, layout[1]);

    if app.create_field == 1 {
        // Approximate cursor position for content (last line)
        let inner_width = layout[1].width.saturating_sub(2) as usize;
        if inner_width > 0 {
            let last_line = app.create_content.lines().last().unwrap_or("");
            let line_count = app.create_content.lines().count().max(1);
            let cursor_x = layout[1].x + 1 + (last_line.len() % inner_width) as u16;
            let cursor_y = layout[1].y + 1 + (line_count as u16).saturating_sub(1);
            frame.set_cursor_position((
                cursor_x.min(layout[1].right() - 2),
                cursor_y.min(layout[1].bottom() - 2),
            ));
        }
    }

    // Kind selector
    let kind_border_style = if app.create_field == 2 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let kind_label = CREATE_KINDS[app.create_kind_index].to_string();
    let kind_block = Block::default()
        .borders(Borders::ALL)
        .border_style(kind_border_style)
        .title(" Kind ");
    let kind_line = Line::from(vec![
        Span::styled(
            " < ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            kind_label,
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " > ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let kind_text = Paragraph::new(kind_line).block(kind_block);
    frame.render_widget(kind_text, layout[2]);

    // Help bar
    frame.render_widget(
        HelpBar {
            screen: &app.screen,
            input_mode: &app.input_mode,
        },
        layout[3],
    );
}
