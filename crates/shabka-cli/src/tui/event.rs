use shabka_core::model::*;
use uuid::Uuid;

/// Actions the UI sends to the async worker task.
#[derive(Debug)]
pub enum AsyncAction {
    /// Load the timeline (initial data fetch).
    LoadTimeline { limit: usize },
    /// Perform a search: embed query → vector_search → rank.
    Search { query: String },
    /// Fetch full detail for a memory (memory + relations + trust).
    LoadDetail { id: Uuid },
}

/// Results the async worker sends back to the UI.
#[derive(Debug)]
pub enum AsyncResult {
    /// Timeline loaded successfully.
    Timeline(Vec<TimelineEntry>),
    /// Search results with scores.
    SearchResults {
        query: String,
        results: Vec<SearchResultEntry>,
    },
    /// Full detail for a single memory.
    Detail {
        memory: Box<Memory>,
        relations: Vec<MemoryRelation>,
        trust: f32,
        history: Vec<String>,
    },
    /// An error occurred during an async operation.
    Error(String),
}

/// A search result entry carrying the memory + its ranked score.
#[derive(Debug, Clone)]
pub struct SearchResultEntry {
    pub memory: Memory,
    pub score: f32,
}
