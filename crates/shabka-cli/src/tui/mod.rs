pub mod app;
pub mod event;
mod views;
mod widgets;

use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self as ct_event, Event};
use ratatui::{DefaultTerminal, Frame};
use shabka_core::config::ShabkaConfig;
use shabka_core::embedding::EmbeddingService;
use shabka_core::history::HistoryLogger;
use shabka_core::model::*;
use shabka_core::ranking::{self, RankCandidate, RankingWeights};
use shabka_core::storage::{Storage, StorageBackend};
use shabka_core::trust;
use tokio::sync::mpsc;

use self::app::{App, Screen};
use self::event::{AsyncAction, AsyncResult, SearchResultEntry};

/// Entry point for the interactive TUI mode.
pub async fn run_tui(config: &ShabkaConfig) -> Result<()> {
    let storage =
        shabka_core::storage::create_backend(config).context("failed to create storage backend")?;
    let embedder = EmbeddingService::from_config(&config.embedding)
        .context("failed to create embedding service")?;

    // Channels for async communication
    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<AsyncAction>();
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<AsyncResult>();

    // Storage info for status view
    let storage_info = config.storage.backend.clone();
    let provider_info = config.embedding.provider.clone();

    // Spawn async worker
    let worker_result_tx = result_tx.clone();
    let history_enabled = config.history.enabled;
    tokio::spawn(async move {
        worker_loop(
            storage,
            embedder,
            history_enabled,
            &mut action_rx,
            &worker_result_tx,
        )
        .await;
    });

    // Fire initial timeline load
    action_tx.send(AsyncAction::LoadTimeline { limit: 500 })?;

    // Initialize terminal
    let mut terminal = ratatui::init();
    let mut app = App::new();

    let result = run_loop(
        &mut terminal,
        &mut app,
        &action_tx,
        &mut result_rx,
        &storage_info,
        &provider_info,
    );

    // Restore terminal
    ratatui::restore();

    result
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action_tx: &mpsc::UnboundedSender<AsyncAction>,
    result_rx: &mut mpsc::UnboundedReceiver<AsyncResult>,
    storage_info: &str,
    provider_info: &str,
) -> Result<()> {
    loop {
        // Draw
        terminal.draw(|frame| render(frame, app, storage_info, provider_info))?;

        // Poll for async results (non-blocking)
        while let Ok(result) = result_rx.try_recv() {
            app.handle_result(result);
        }

        // If a save/update completed, trigger a timeline refresh
        if app.needs_refresh {
            app.needs_refresh = false;
            let _ = action_tx.send(AsyncAction::LoadTimeline { limit: 500 });
        }

        // Poll for keyboard events (50ms timeout for responsive UI)
        if ct_event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = ct_event::read()? {
                if let Some(action) = app.handle_key(key) {
                    let _ = action_tx.send(action);
                }
            }
        }

        // Tick error timer
        app.tick_error();

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn render(frame: &mut Frame, app: &App, storage_info: &str, provider_info: &str) {
    let area = frame.area();

    // Show splash during initial load or minimum display time
    let splash_active = std::time::Instant::now() < app.splash_until;
    if (app.loading || splash_active) && app.screen == Screen::List && app.active_query.is_none() {
        views::splash::render(
            frame,
            area,
            storage_info,
            provider_info,
            app.entries.len(),
            app.loading,
        );
        return;
    }

    match app.screen {
        Screen::List => views::list::render(frame, app, area),
        Screen::Detail => views::detail::render(frame, app, area),
        Screen::Status => views::status::render(frame, app, area, storage_info, provider_info),
        Screen::Create => views::create::render(frame, app, area),
    }

    // Render error toast overlay if present
    if let Some(ref msg) = app.error_message {
        render_error_toast(frame, msg);
    }
}

fn render_error_toast(frame: &mut Frame, msg: &str) {
    use ratatui::{
        layout::{Constraint, Flex, Layout},
        style::{Color, Style},
        widgets::{Block, Borders, Clear, Paragraph},
    };

    let area = frame.area();
    let [toast_area] = Layout::horizontal([Constraint::Percentage(60)])
        .flex(Flex::Center)
        .areas(area);
    let [toast_area] = Layout::vertical([Constraint::Length(3)])
        .flex(Flex::End)
        .areas(toast_area);

    frame.render_widget(Clear, toast_area);
    let toast = Paragraph::new(format!(" ✗ {msg}"))
        .style(Style::default().fg(Color::White).bg(Color::Red))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title(" Error "),
        );
    frame.render_widget(toast, toast_area);
}

/// Async worker loop: processes actions using the storage + embedder.
async fn worker_loop(
    storage: Storage,
    embedder: EmbeddingService,
    history_enabled: bool,
    action_rx: &mut mpsc::UnboundedReceiver<AsyncAction>,
    result_tx: &mpsc::UnboundedSender<AsyncResult>,
) {
    let history = HistoryLogger::new(history_enabled);

    while let Some(action) = action_rx.recv().await {
        let result = match action {
            AsyncAction::LoadTimeline { limit } => {
                let query = TimelineQuery {
                    limit,
                    ..Default::default()
                };
                match storage.timeline(&query).await {
                    Ok(entries) => AsyncResult::Timeline(entries),
                    Err(e) => AsyncResult::Error(format!("Failed to load timeline: {e}")),
                }
            }
            AsyncAction::Search { query } => match do_search(&storage, &embedder, &query).await {
                Ok(results) => AsyncResult::SearchResults { query, results },
                Err(e) => AsyncResult::Error(format!("Search failed: {e}")),
            },
            AsyncAction::LoadDetail { id } => match do_load_detail(&storage, &history, id).await {
                Ok((memory, relations, trust_val, hist)) => AsyncResult::Detail {
                    memory: Box::new(memory),
                    relations,
                    trust: trust_val,
                    history: hist,
                },
                Err(e) => AsyncResult::Error(format!("Failed to load detail: {e}")),
            },
            AsyncAction::SaveMemory {
                title,
                content,
                kind,
            } => {
                let memory = Memory::new(title, content, kind, "tui".to_string());
                match storage.save_memory(&memory, None).await {
                    Ok(()) => AsyncResult::MemorySaved,
                    Err(e) => AsyncResult::Error(format!("Failed to save memory: {e}")),
                }
            }
            AsyncAction::UpdateMemory {
                id,
                title,
                content,
                kind,
            } => {
                let input = UpdateMemoryInput {
                    title: Some(title),
                    content: Some(content),
                    kind: Some(kind),
                    ..Default::default()
                };
                match storage.update_memory(id, &input).await {
                    Ok(_) => AsyncResult::MemoryUpdated,
                    Err(e) => AsyncResult::Error(format!("Failed to update memory: {e}")),
                }
            }
        };
        if result_tx.send(result).is_err() {
            break; // UI closed
        }
    }
}

async fn do_search(
    storage: &Storage,
    embedder: &EmbeddingService,
    query: &str,
) -> Result<Vec<SearchResultEntry>> {
    let embedding = embedder
        .embed(query)
        .await
        .context("failed to embed search query")?;

    let results = storage
        .vector_search(&embedding, 50)
        .await
        .context("vector search failed")?;

    if results.is_empty() {
        return Ok(Vec::new());
    }

    let memory_ids: Vec<_> = results.iter().map(|(m, _)| m.id).collect();
    let relation_counts = storage
        .count_relations(&memory_ids)
        .await
        .unwrap_or_default();
    let contradiction_counts = storage
        .count_contradictions(&memory_ids)
        .await
        .unwrap_or_default();

    let rel_map: std::collections::HashMap<_, _> = relation_counts.into_iter().collect();
    let contra_map: std::collections::HashMap<_, _> = contradiction_counts.into_iter().collect();

    let candidates: Vec<RankCandidate> = results
        .into_iter()
        .map(|(memory, score)| {
            let keyword_score = ranking::keyword_score(query, &memory);
            RankCandidate {
                vector_score: score,
                keyword_score,
                relation_count: rel_map.get(&memory.id).copied().unwrap_or(0),
                contradiction_count: contra_map.get(&memory.id).copied().unwrap_or(0),
                memory,
            }
        })
        .collect();

    let ranked = ranking::rank(candidates, &RankingWeights::default());

    Ok(ranked
        .into_iter()
        .take(20)
        .map(|r| SearchResultEntry {
            score: r.score,
            memory: r.memory,
        })
        .collect())
}

async fn do_load_detail(
    storage: &Storage,
    history: &HistoryLogger,
    id: uuid::Uuid,
) -> Result<(Memory, Vec<MemoryRelation>, f32, Vec<String>)> {
    let memory = storage
        .get_memory(id)
        .await
        .context("failed to load memory")?;

    let relations = storage.get_relations(id).await.unwrap_or_default();

    let contradiction_count = storage
        .count_contradictions(&[id])
        .await
        .unwrap_or_default()
        .into_iter()
        .next()
        .map(|(_, c)| c)
        .unwrap_or(0);

    let trust_val = trust::trust_score(&memory, contradiction_count);

    let hist_events = history.history_for(id);
    let hist_strings: Vec<String> = hist_events
        .iter()
        .map(|e| {
            format!(
                "{} — {} by {}",
                e.timestamp.format("%Y-%m-%d %H:%M"),
                e.action,
                e.actor,
            )
        })
        .collect();

    Ok((memory, relations, trust_val, hist_strings))
}
