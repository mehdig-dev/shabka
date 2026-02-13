//! Append-only audit trail for memory mutations.
//!
//! Events are stored as JSONL at `~/.config/kaizen/history.jsonl`.
//! Each line is a self-contained [`MemoryEvent`] that records who did what and when.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

use crate::model::{Memory, UpdateMemoryInput};

/// What happened to the memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventAction {
    Created,
    Updated,
    Deleted,
    Archived,
    Imported,
    Superseded,
}

impl std::fmt::Display for EventAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Updated => write!(f, "updated"),
            Self::Deleted => write!(f, "deleted"),
            Self::Archived => write!(f, "archived"),
            Self::Imported => write!(f, "imported"),
            Self::Superseded => write!(f, "superseded"),
        }
    }
}

/// A single field change in an update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

/// A single audit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub action: EventAction,
    pub actor: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<FieldChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_title: Option<String>,
}

impl MemoryEvent {
    pub fn new(memory_id: Uuid, action: EventAction, actor: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            memory_id,
            action,
            actor,
            timestamp: Utc::now(),
            changes: Vec::new(),
            memory_title: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.memory_title = Some(title.into());
        self
    }

    pub fn with_changes(mut self, changes: Vec<FieldChange>) -> Self {
        self.changes = changes;
        self
    }
}

/// Append-only JSONL logger for memory events.
pub struct HistoryLogger {
    path: PathBuf,
    enabled: bool,
}

impl HistoryLogger {
    pub fn new(enabled: bool) -> Self {
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kaizen")
            .join("history.jsonl");
        Self { path, enabled }
    }

    /// Log a single event by appending one JSON line.
    pub fn log(&self, event: &MemoryEvent) {
        if !self.enabled {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let line = match serde_json::to_string(event) {
            Ok(l) => l,
            Err(e) => {
                tracing::debug!("history: failed to serialize event: {e}");
                return;
            }
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path);
        match file {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", line);
            }
            Err(e) => {
                tracing::debug!("history: failed to open log: {e}");
            }
        }
    }

    /// Get all events for a specific memory, most recent first.
    pub fn history_for(&self, memory_id: Uuid) -> Vec<MemoryEvent> {
        let mut events = self.read_all();
        events.retain(|e| e.memory_id == memory_id);
        events.reverse();
        events
    }

    /// Get the N most recent events across all memories.
    pub fn recent(&self, limit: usize) -> Vec<MemoryEvent> {
        let mut events = self.read_all();
        events.reverse();
        events.truncate(limit);
        events
    }

    fn read_all(&self) -> Vec<MemoryEvent> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}

/// Compute field-level diffs between the old memory and an update input.
pub fn diff_update(old: &Memory, input: &UpdateMemoryInput) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    if let Some(ref new_title) = input.title {
        if *new_title != old.title {
            changes.push(FieldChange {
                field: "title".to_string(),
                old_value: old.title.clone(),
                new_value: new_title.clone(),
            });
        }
    }
    if let Some(ref new_content) = input.content {
        if *new_content != old.content {
            changes.push(FieldChange {
                field: "content".to_string(),
                old_value: format!("({} chars)", old.content.len()),
                new_value: format!("({} chars)", new_content.len()),
            });
        }
    }
    if let Some(ref new_tags) = input.tags {
        let old_tags = old.tags.join(", ");
        let new_tags_str = new_tags.join(", ");
        if old_tags != new_tags_str {
            changes.push(FieldChange {
                field: "tags".to_string(),
                old_value: old_tags,
                new_value: new_tags_str,
            });
        }
    }
    if let Some(new_importance) = input.importance {
        if (new_importance - old.importance).abs() > f32::EPSILON {
            changes.push(FieldChange {
                field: "importance".to_string(),
                old_value: format!("{:.2}", old.importance),
                new_value: format!("{:.2}", new_importance),
            });
        }
    }
    if let Some(ref new_status) = input.status {
        if *new_status != old.status {
            changes.push(FieldChange {
                field: "status".to_string(),
                old_value: old.status.to_string(),
                new_value: new_status.to_string(),
            });
        }
    }
    if let Some(ref new_privacy) = input.privacy {
        if *new_privacy != old.privacy {
            changes.push(FieldChange {
                field: "privacy".to_string(),
                old_value: old.privacy.to_string(),
                new_value: new_privacy.to_string(),
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MemoryKind, MemoryStatus};

    #[test]
    fn test_event_serde_roundtrip() {
        let event = MemoryEvent::new(Uuid::nil(), EventAction::Created, "alice".to_string())
            .with_title("Test memory");
        let json = serde_json::to_string(&event).unwrap();
        let parsed: MemoryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.memory_id, Uuid::nil());
        assert_eq!(parsed.action, EventAction::Created);
        assert_eq!(parsed.actor, "alice");
        assert_eq!(parsed.memory_title.as_deref(), Some("Test memory"));
    }

    #[test]
    fn test_diff_update_detects_changes() {
        let old = Memory::new(
            "Old Title".to_string(),
            "Old content".to_string(),
            MemoryKind::Observation,
            "user".to_string(),
        );
        let input = UpdateMemoryInput {
            title: Some("New Title".to_string()),
            importance: Some(0.9),
            ..Default::default()
        };
        let changes = diff_update(&old, &input);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].field, "title");
        assert_eq!(changes[1].field, "importance");
    }

    #[test]
    fn test_diff_update_no_changes() {
        let old = Memory::new(
            "Same".to_string(),
            "Content".to_string(),
            MemoryKind::Fact,
            "user".to_string(),
        );
        let input = UpdateMemoryInput {
            title: Some("Same".to_string()),
            ..Default::default()
        };
        let changes = diff_update(&old, &input);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_action_display() {
        assert_eq!(EventAction::Created.to_string(), "created");
        assert_eq!(EventAction::Updated.to_string(), "updated");
        assert_eq!(EventAction::Deleted.to_string(), "deleted");
        assert_eq!(EventAction::Archived.to_string(), "archived");
        assert_eq!(EventAction::Imported.to_string(), "imported");
        assert_eq!(EventAction::Superseded.to_string(), "superseded");
    }

    #[test]
    fn test_disabled_logger_noop() {
        let logger = HistoryLogger::new(false);
        let event = MemoryEvent::new(Uuid::nil(), EventAction::Created, "user".to_string());
        // Should not panic or do anything
        logger.log(&event);
    }

    #[test]
    fn test_diff_update_status_change() {
        let old = Memory::new(
            "T".to_string(),
            "C".to_string(),
            MemoryKind::Error,
            "user".to_string(),
        );
        let input = UpdateMemoryInput {
            status: Some(MemoryStatus::Archived),
            ..Default::default()
        };
        let changes = diff_update(&old, &input);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "status");
        assert_eq!(changes[0].old_value, "active");
        assert_eq!(changes[0].new_value, "archived");
    }
}
