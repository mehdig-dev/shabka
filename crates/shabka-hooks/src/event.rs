use serde::Deserialize;
use shabka_core::model::MemoryKind;

/// JSON payload received from Claude Code hooks on stdin.
///
/// Fields vary by event type — tool-related fields are only present
/// for PostToolUse / PostToolUseFailure events.
#[derive(Debug, Clone, Deserialize)]
pub struct HookEvent {
    pub session_id: String,
    pub cwd: String,
    pub hook_event_name: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_output: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Present on Stop events; not used in classification but part of the wire format.
    #[serde(default)]
    #[allow(dead_code)]
    pub stop_hook_active: Option<bool>,
    /// Present on UserPromptSubmit events — the user's prompt text.
    #[serde(default)]
    pub prompt: Option<String>,
}

/// Result of classifying a hook event.
pub enum CaptureIntent {
    /// Save a memory with these fields.
    Save {
        kind: MemoryKind,
        title: String,
        content: String,
        importance: f32,
        tags: Vec<String>,
    },
    /// Skip this event — not worth capturing.
    Skip { reason: String },
    /// Buffer this event for session compression (don't save immediately).
    Buffer {
        kind: MemoryKind,
        title: String,
        content: String,
        importance: f32,
        tags: Vec<String>,
        file_path: Option<String>,
        event_type: String,
    },
}
