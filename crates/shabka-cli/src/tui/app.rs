use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use shabka_core::model::*;

use super::event::{AsyncAction, AsyncResult, SearchResultEntry};

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    List,
    Detail,
    Status,
    Create,
}

/// Input mode within the current screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Filter,
}

/// All memory kinds for create/edit form.
pub const CREATE_KINDS: &[MemoryKind] = &[
    MemoryKind::Observation,
    MemoryKind::Decision,
    MemoryKind::Pattern,
    MemoryKind::Error,
    MemoryKind::Fix,
    MemoryKind::Preference,
    MemoryKind::Fact,
    MemoryKind::Lesson,
    MemoryKind::Todo,
    MemoryKind::Procedure,
];

/// All memory kinds for filter cycling.
pub const ALL_KINDS: &[Option<MemoryKind>] = &[
    None, // "All"
    Some(MemoryKind::Observation),
    Some(MemoryKind::Decision),
    Some(MemoryKind::Pattern),
    Some(MemoryKind::Error),
    Some(MemoryKind::Fix),
    Some(MemoryKind::Preference),
    Some(MemoryKind::Fact),
    Some(MemoryKind::Lesson),
    Some(MemoryKind::Todo),
    Some(MemoryKind::Procedure),
];

/// Central application state.
pub struct App {
    pub screen: Screen,
    pub input_mode: InputMode,
    pub should_quit: bool,
    pub loading: bool,
    pub needs_refresh: bool,

    // -- List state --
    pub entries: Vec<TimelineEntry>,
    pub filtered_entries: Vec<usize>, // indices into `entries`
    pub selected: usize,
    pub search_input: String,
    pub search_cursor: usize,
    pub active_query: Option<String>,
    pub search_results: Vec<SearchResultEntry>,
    pub filter_kind_index: usize, // index into ALL_KINDS

    // -- Detail state --
    pub detail_memory: Option<Memory>,
    pub detail_relations: Vec<MemoryRelation>,
    pub detail_trust: f32,
    pub detail_history: Vec<String>,
    pub detail_scroll: u16,

    // -- Status state --
    pub kind_counts: Vec<(String, usize)>,

    // -- Splash --
    pub splash_until: std::time::Instant,

    // -- Create/Edit form state --
    pub create_title: String,
    pub create_content: String,
    pub create_kind_index: usize,
    pub create_field: usize, // 0=title, 1=content, 2=kind
    pub editing_id: Option<uuid::Uuid>,

    // -- Error toast --
    pub error_message: Option<String>,
    pub error_timer: u8, // ticks remaining
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::List,
            input_mode: InputMode::Normal,
            should_quit: false,
            loading: true,
            needs_refresh: false,

            entries: Vec::new(),
            filtered_entries: Vec::new(),
            selected: 0,
            search_input: String::new(),
            search_cursor: 0,
            active_query: None,
            search_results: Vec::new(),
            filter_kind_index: 0,

            detail_memory: None,
            detail_relations: Vec::new(),
            detail_trust: 0.0,
            detail_history: Vec::new(),
            detail_scroll: 0,

            kind_counts: Vec::new(),

            splash_until: std::time::Instant::now() + std::time::Duration::from_secs(3),

            create_title: String::new(),
            create_content: String::new(),
            create_kind_index: 0,
            create_field: 0,
            editing_id: None,

            error_message: None,
            error_timer: 0,
        }
    }

    /// Process an async result from the worker.
    pub fn handle_result(&mut self, result: AsyncResult) {
        match result {
            AsyncResult::Timeline(entries) => {
                self.entries = entries;
                self.refilter();
                self.loading = false;
            }
            AsyncResult::SearchResults { query, results } => {
                self.active_query = Some(query);
                self.search_results = results;
                self.loading = false;
                // Reset selection
                self.selected = 0;
            }
            AsyncResult::Detail {
                memory,
                relations,
                trust,
                history,
            } => {
                self.detail_memory = Some(*memory);
                self.detail_relations = relations;
                self.detail_trust = trust;
                self.detail_history = history;
                self.detail_scroll = 0;
                self.screen = Screen::Detail;
                self.loading = false;
            }
            AsyncResult::MemorySaved | AsyncResult::MemoryUpdated => {
                self.screen = Screen::List;
                self.editing_id = None;
                self.loading = true;
                // The caller will need to trigger a timeline refresh.
                // We set a flag via `loading` so the next handle cycle picks it up.
                self.needs_refresh = true;
            }
            AsyncResult::Error(msg) => {
                self.error_message = Some(msg);
                self.error_timer = 100; // ~5s at 50ms tick
                self.loading = false;
            }
        }
    }

    /// Handle a key event. Returns an optional async action to dispatch.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }

        match (&self.screen, &self.input_mode) {
            (Screen::List, InputMode::Normal) => self.handle_list_normal(key),
            (Screen::List, InputMode::Search) => self.handle_list_search(key),
            (Screen::List, InputMode::Filter) => self.handle_list_filter(key),
            (Screen::Detail, InputMode::Normal) => self.handle_detail_normal(key),
            (Screen::Status, InputMode::Normal) => {
                self.handle_status_normal(key);
                None
            }
            (Screen::Create, _) => self.handle_create(key),
            _ => None,
        }
    }

    fn handle_list_normal(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('G') => {
                let len = self.visible_count();
                if len > 0 {
                    self.selected = len - 1;
                }
                None
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                None
            }
            KeyCode::PageDown => {
                self.move_selection(20);
                None
            }
            KeyCode::PageUp => {
                self.move_selection(-20);
                None
            }
            KeyCode::Enter => self.open_detail(),
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                self.search_input.clear();
                self.search_cursor = 0;
                None
            }
            KeyCode::Char('f') => {
                self.input_mode = InputMode::Filter;
                None
            }
            KeyCode::Tab => {
                self.screen = Screen::Status;
                self.compute_kind_counts();
                None
            }
            KeyCode::Char('r') => {
                // Refresh
                self.loading = true;
                Some(AsyncAction::LoadTimeline { limit: 500 })
            }
            KeyCode::Char('n') => {
                // Open create screen with blank form
                self.create_title.clear();
                self.create_content.clear();
                self.create_kind_index = 0;
                self.create_field = 0;
                self.editing_id = None;
                self.screen = Screen::Create;
                None
            }
            KeyCode::Esc => {
                // Clear search results, go back to timeline
                if self.active_query.is_some() {
                    self.active_query = None;
                    self.search_results.clear();
                    self.selected = 0;
                }
                None
            }
            _ => None,
        }
    }

    fn handle_list_search(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                None
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                if self.search_input.trim().is_empty() {
                    // Clear search
                    self.active_query = None;
                    self.search_results.clear();
                    None
                } else {
                    self.loading = true;
                    Some(AsyncAction::Search {
                        query: self.search_input.clone(),
                    })
                }
            }
            KeyCode::Backspace => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                    self.search_input.remove(self.search_cursor);
                }
                None
            }
            KeyCode::Left => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                }
                None
            }
            KeyCode::Right => {
                if self.search_cursor < self.search_input.len() {
                    self.search_cursor += 1;
                }
                None
            }
            KeyCode::Char(c) => {
                self.search_input.insert(self.search_cursor, c);
                self.search_cursor += 1;
                None
            }
            _ => None,
        }
    }

    fn handle_list_filter(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('f') => {
                self.input_mode = InputMode::Normal;
                self.refilter();
                self.selected = 0;
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.filter_kind_index = (self.filter_kind_index + 1) % ALL_KINDS.len();
                self.refilter();
                self.selected = 0;
                None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.filter_kind_index == 0 {
                    self.filter_kind_index = ALL_KINDS.len() - 1;
                } else {
                    self.filter_kind_index -= 1;
                }
                self.refilter();
                self.selected = 0;
                None
            }
            _ => None,
        }
    }

    fn handle_detail_normal(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Char('e') => {
                // Open create screen pre-filled with current memory for editing
                if let Some(ref memory) = self.detail_memory {
                    self.create_title = memory.title.clone();
                    self.create_content = memory.content.clone();
                    self.create_kind_index = CREATE_KINDS
                        .iter()
                        .position(|k| *k == memory.kind)
                        .unwrap_or(0);
                    self.create_field = 0;
                    self.editing_id = Some(memory.id);
                    self.screen = Screen::Create;
                }
                None
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.screen = Screen::List;
                self.detail_memory = None;
                self.detail_relations.clear();
                self.detail_history.clear();
                self.detail_scroll = 0;
                None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
                None
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(20);
                None
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(20);
                None
            }
            _ => None,
        }
    }

    fn handle_status_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc | KeyCode::Tab => {
                self.screen = Screen::List;
            }
            _ => {}
        }
    }

    fn handle_create(&mut self, key: KeyEvent) -> Option<AsyncAction> {
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::List;
                self.editing_id = None;
                None
            }
            KeyCode::Tab => {
                self.create_field = (self.create_field + 1) % 3;
                None
            }
            KeyCode::BackTab => {
                self.create_field = if self.create_field == 0 {
                    2
                } else {
                    self.create_field - 1
                };
                None
            }
            KeyCode::Up if self.create_field == 2 => {
                // Cycle kind backwards
                if self.create_kind_index == 0 {
                    self.create_kind_index = CREATE_KINDS.len() - 1;
                } else {
                    self.create_kind_index -= 1;
                }
                None
            }
            KeyCode::Down if self.create_field == 2 => {
                // Cycle kind forward
                self.create_kind_index = (self.create_kind_index + 1) % CREATE_KINDS.len();
                None
            }
            KeyCode::Enter if self.create_field == 2 => {
                // Cycle kind forward on Enter
                self.create_kind_index = (self.create_kind_index + 1) % CREATE_KINDS.len();
                None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+S: Save memory
                if self.create_title.trim().is_empty() {
                    return None;
                }
                let kind = CREATE_KINDS[self.create_kind_index];
                if let Some(id) = self.editing_id {
                    Some(AsyncAction::UpdateMemory {
                        id,
                        title: self.create_title.clone(),
                        content: self.create_content.clone(),
                        kind,
                    })
                } else {
                    Some(AsyncAction::SaveMemory {
                        title: self.create_title.clone(),
                        content: self.create_content.clone(),
                        kind,
                    })
                }
            }
            KeyCode::Char(c) if self.create_field < 2 => {
                if self.create_field == 0 {
                    self.create_title.push(c);
                } else {
                    self.create_content.push(c);
                }
                None
            }
            KeyCode::Backspace if self.create_field < 2 => {
                if self.create_field == 0 {
                    self.create_title.pop();
                } else {
                    self.create_content.pop();
                }
                None
            }
            KeyCode::Enter if self.create_field == 1 => {
                // Newline in content field
                self.create_content.push('\n');
                None
            }
            _ => None,
        }
    }

    fn open_detail(&mut self) -> Option<AsyncAction> {
        let id = if self.active_query.is_some() {
            // Browsing search results
            self.search_results.get(self.selected).map(|r| r.memory.id)
        } else {
            // Browsing timeline
            self.filtered_entries
                .get(self.selected)
                .and_then(|&idx| self.entries.get(idx))
                .map(|e| e.id)
        };

        if let Some(id) = id {
            self.loading = true;
            Some(AsyncAction::LoadDetail { id })
        } else {
            None
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.visible_count();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected as i32;
        let new = (current + delta).clamp(0, len as i32 - 1);
        self.selected = new as usize;
    }

    /// How many items are currently visible in the list.
    pub fn visible_count(&self) -> usize {
        if self.active_query.is_some() {
            self.search_results.len()
        } else {
            self.filtered_entries.len()
        }
    }

    /// Recompute filtered_entries based on the current kind filter.
    pub fn refilter(&mut self) {
        let kind_filter = ALL_KINDS[self.filter_kind_index];
        self.filtered_entries = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if let Some(k) = kind_filter {
                    e.kind == k
                } else {
                    true
                }
            })
            .map(|(i, _)| i)
            .collect();
    }

    /// Compute kind counts for the status view.
    fn compute_kind_counts(&mut self) {
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for entry in &self.entries {
            *counts.entry(entry.kind.to_string()).or_default() += 1;
        }
        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        self.kind_counts = sorted;
    }

    /// Current filter label for display (used in tests and by filter_bar widget).
    #[allow(dead_code)]
    pub fn filter_label(&self) -> &str {
        match ALL_KINDS[self.filter_kind_index] {
            None => "All",
            Some(MemoryKind::Observation) => "Observation",
            Some(MemoryKind::Decision) => "Decision",
            Some(MemoryKind::Pattern) => "Pattern",
            Some(MemoryKind::Error) => "Error",
            Some(MemoryKind::Fix) => "Fix",
            Some(MemoryKind::Preference) => "Preference",
            Some(MemoryKind::Fact) => "Fact",
            Some(MemoryKind::Lesson) => "Lesson",
            Some(MemoryKind::Todo) => "Todo",
            Some(MemoryKind::Procedure) => "Procedure",
        }
    }

    /// Tick the error timer down.
    pub fn tick_error(&mut self) {
        if self.error_timer > 0 {
            self.error_timer -= 1;
            if self.error_timer == 0 {
                self.error_message = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_initial_state() {
        let app = App::new();
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit);
        assert!(app.loading);
    }

    #[test]
    fn test_quit() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn test_selection_navigation() {
        let mut app = App::new();
        app.loading = false;
        // Add some fake entries
        for i in 0..5 {
            app.entries.push(TimelineEntry {
                id: uuid::Uuid::now_v7(),
                title: format!("Memory {i}"),
                kind: MemoryKind::Observation,
                summary: String::new(),
                importance: 0.5,
                created_at: chrono::Utc::now(),
                session_id: None,
                related_count: 0,
                privacy: MemoryPrivacy::Private,
                created_by: "test".into(),
                project_id: None,
                status: MemoryStatus::Active,
                verification: VerificationStatus::Unverified,
            });
        }
        app.refilter();

        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 2);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 1);
        // Can't go below 0
        app.handle_key(key(KeyCode::Char('k')));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_search_mode_toggle() {
        let mut app = App::new();
        app.loading = false;
        assert_eq!(app.input_mode, InputMode::Normal);

        app.handle_key(key(KeyCode::Char('/')));
        assert_eq!(app.input_mode, InputMode::Search);

        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_search_input_typing() {
        let mut app = App::new();
        app.loading = false;
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('h')));
        app.handle_key(key(KeyCode::Char('i')));
        assert_eq!(app.search_input, "hi");
        assert_eq!(app.search_cursor, 2);

        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.search_input, "h");
        assert_eq!(app.search_cursor, 1);
    }

    #[test]
    fn test_filter_mode_cycling() {
        let mut app = App::new();
        app.loading = false;
        app.handle_key(key(KeyCode::Char('f')));
        assert_eq!(app.input_mode, InputMode::Filter);
        assert_eq!(app.filter_kind_index, 0);
        assert_eq!(app.filter_label(), "All");

        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.filter_kind_index, 1);
        assert_eq!(app.filter_label(), "Observation");

        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.filter_kind_index, 0);
        assert_eq!(app.filter_label(), "All");

        // Wrap around left
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.filter_label(), "Procedure");
    }

    #[test]
    fn test_tab_to_status() {
        let mut app = App::new();
        app.loading = false;
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.screen, Screen::Status);

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.screen, Screen::List);
    }

    #[test]
    fn test_detail_scroll() {
        let mut app = App::new();
        app.screen = Screen::Detail;
        app.detail_scroll = 5;

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.detail_scroll, 6);

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.detail_scroll, 5);
    }

    #[test]
    fn test_detail_back_to_list() {
        let mut app = App::new();
        app.screen = Screen::Detail;

        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::List);
    }

    #[test]
    fn test_ctrl_c_quits() {
        let mut app = App::new();
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        app.handle_key(key);
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_timeline_result() {
        let mut app = App::new();
        assert!(app.loading);

        let entries = vec![TimelineEntry {
            id: uuid::Uuid::now_v7(),
            title: "Test".into(),
            kind: MemoryKind::Fact,
            summary: String::new(),
            importance: 0.8,
            created_at: chrono::Utc::now(),
            session_id: None,
            related_count: 0,
            privacy: MemoryPrivacy::Private,
            created_by: "test".into(),
            project_id: None,
            status: MemoryStatus::Active,
            verification: VerificationStatus::Verified,
        }];

        app.handle_result(super::super::event::AsyncResult::Timeline(entries));
        assert!(!app.loading);
        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.filtered_entries.len(), 1);
    }

    #[test]
    fn test_error_toast_timer() {
        let mut app = App::new();
        app.handle_result(super::super::event::AsyncResult::Error("test error".into()));
        assert!(app.error_message.is_some());
        assert_eq!(app.error_timer, 100);

        for _ in 0..99 {
            app.tick_error();
        }
        assert!(app.error_message.is_some());

        app.tick_error();
        assert!(app.error_message.is_none());
        assert_eq!(app.error_timer, 0);
    }
}
