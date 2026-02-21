use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row, Table, TableState},
    Frame,
};
use shabka_core::model::VerificationStatus;

use crate::tui::{
    app::{App, InputMode},
    widgets::{filter_bar::FilterBar, help_bar::HelpBar, search_input::SearchInput},
};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([
        Constraint::Length(3), // search bar
        Constraint::Length(1), // filter bar
        Constraint::Min(5),    // table
        Constraint::Length(1), // help bar
    ])
    .split(area);

    // Search bar
    frame.render_widget(
        SearchInput {
            text: &app.search_input,
            cursor: app.search_cursor,
            focused: app.input_mode == InputMode::Search,
        },
        layout[0],
    );

    // Filter bar
    frame.render_widget(
        FilterBar {
            selected_index: app.filter_kind_index,
            active: app.input_mode == InputMode::Filter,
        },
        layout[1],
    );

    // Table
    render_table(frame, app, layout[2]);

    // Help bar
    frame.render_widget(
        HelpBar {
            screen: &app.screen,
            input_mode: &app.input_mode,
        },
        layout[3],
    );
}

fn render_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.loading {
        let loading = Line::from(vec![Span::styled(
            "  Loading...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]);
        frame.render_widget(loading, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Kind"),
        Cell::from("Imp%"),
        Cell::from("Status"),
        Cell::from("Title"),
        Cell::from("Created"),
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = if app.active_query.is_some() {
        // Show search results
        app.search_results
            .iter()
            .map(|result| {
                let m = &result.memory;
                make_memory_row(
                    m.id.to_string()[..8].to_string(),
                    m.kind.to_string(),
                    m.importance,
                    &m.verification,
                    &m.title,
                    m.created_at.format("%Y-%m-%d").to_string(),
                    Some(result.score),
                )
            })
            .collect()
    } else if app.filtered_entries.is_empty() {
        // Empty state
        vec![Row::new(vec![Cell::from(Span::styled(
            "  No memories found. Press / to search or r to refresh.",
            Style::default().fg(Color::DarkGray),
        ))])]
    } else {
        app.filtered_entries
            .iter()
            .map(|&idx| {
                let entry = &app.entries[idx];
                make_memory_row(
                    entry.id.to_string()[..8].to_string(),
                    entry.kind.to_string(),
                    entry.importance,
                    &entry.verification,
                    &entry.title,
                    entry.created_at.format("%Y-%m-%d").to_string(),
                    None,
                )
            })
            .collect()
    };

    let widths = [
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(12),
    ];

    let title = if let Some(ref q) = app.active_query {
        format!(" Results for \"{}\" ({}) ", q, app.search_results.len())
    } else {
        format!(" Memories ({}) ", app.filtered_entries.len())
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(title),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::Indexed(236)) // subtle dark bg (#303030)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = TableState::default();
    state.select(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn make_memory_row(
    id: String,
    kind: String,
    importance: f32,
    verification: &VerificationStatus,
    title: &str,
    date: String,
    score: Option<f32>,
) -> Row<'static> {
    let id_cell = Cell::from(Span::styled(id, Style::default().fg(Color::Cyan)));

    let kind_cell = Cell::from(Span::styled(kind, Style::default().fg(Color::Magenta)));

    let imp_color = if importance >= 0.7 {
        Color::Green
    } else if importance >= 0.4 {
        Color::Yellow
    } else {
        Color::Red
    };
    let imp_text = if let Some(s) = score {
        format!("{:.0}%", s * 100.0)
    } else {
        format!("{:.0}%", importance * 100.0)
    };
    let imp_cell = Cell::from(Span::styled(imp_text, Style::default().fg(imp_color)));

    let (ver_text, ver_color) = match verification {
        VerificationStatus::Verified => ("✓ verified", Color::Green),
        VerificationStatus::Disputed => ("✗ disputed", Color::Red),
        VerificationStatus::Outdated => ("⚠ outdated", Color::Yellow),
        VerificationStatus::Unverified => ("  —", Color::DarkGray),
    };
    let ver_cell = Cell::from(Span::styled(ver_text, Style::default().fg(ver_color)));

    // Truncate title if too long
    let max_title = 60;
    let display_title = if title.len() > max_title {
        format!("{}…", &title[..max_title - 1])
    } else {
        title.to_string()
    };
    let title_cell = Cell::from(display_title);

    let date_cell = Cell::from(Span::styled(date, Style::default().fg(Color::DarkGray)));

    Row::new(vec![
        id_cell, kind_cell, imp_cell, ver_cell, title_cell, date_cell,
    ])
}
