use shabka_core::model::MemoryKind;

use crate::event::{CaptureIntent, HookEvent};

/// Classify a hook event into a capture intent.
/// When `session_compression` is true, PostToolUse/PostToolUseFailure events
/// return Buffer instead of Save, so they can be compressed at session end.
pub fn classify(event: &HookEvent, session_compression: bool) -> CaptureIntent {
    match event.hook_event_name.as_str() {
        "PostToolUse" => classify_post_tool_use(event, session_compression),
        "PostToolUseFailure" => classify_failure(event, session_compression),
        "Stop" => CaptureIntent::Skip {
            reason: "Stop events are handled separately".into(),
        },
        "UserPromptSubmit" => classify_user_prompt(event),
        other => CaptureIntent::Skip {
            reason: format!("unhandled event type: {other}"),
        },
    }
}

/// PostToolUse: capture file edits (Edit/Write) and failed Bash commands.
fn classify_post_tool_use(event: &HookEvent, session_compression: bool) -> CaptureIntent {
    let tool = event.tool_name.as_deref().unwrap_or("");
    match tool {
        "Edit" | "Write" => classify_file_change(event, tool, session_compression),
        "Bash" => classify_bash_output(event, session_compression),
        _ => CaptureIntent::Skip {
            reason: format!("PostToolUse for untracked tool: {tool}"),
        },
    }
}

/// File change via Edit or Write — capture as a decision.
fn classify_file_change(event: &HookEvent, tool: &str, session_compression: bool) -> CaptureIntent {
    let file_path = event
        .tool_input
        .as_ref()
        .and_then(|v| v.get("file_path").or(v.get("filePath")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown file");

    let filename = basename(file_path);

    // Build a meaningful title from the change, not just the path
    let title = if tool == "Edit" {
        // Try to summarize what changed from old/new strings
        let summary = event.tool_input.as_ref().and_then(|input| {
            let new = input.get("new_string").and_then(|v| v.as_str())?;
            // Extract first meaningful line from the new content
            let first_line = new.lines().find(|l| !l.trim().is_empty()).unwrap_or(new);
            Some(truncate(first_line.trim(), 50))
        });
        match summary {
            Some(s) => format!("Edit {filename}: {s}"),
            None => format!("Edit {filename}"),
        }
    } else {
        format!("Write {filename}")
    };

    let mut content = format!("File modified via {tool}: {file_path}");

    // For Edit, include the old/new strings if available
    if tool == "Edit" {
        if let Some(input) = &event.tool_input {
            if let Some(old) = input.get("old_string").and_then(|v| v.as_str()) {
                let old_preview = truncate(old, 500);
                content.push_str(&format!("\n\nReplaced:\n```\n{old_preview}\n```"));
            }
            if let Some(new) = input.get("new_string").and_then(|v| v.as_str()) {
                let new_preview = truncate(new, 500);
                content.push_str(&format!("\n\nWith:\n```\n{new_preview}\n```"));
            }
        }
    }

    if session_compression {
        CaptureIntent::Buffer {
            kind: MemoryKind::Decision,
            title,
            content,
            importance: 0.4,
            tags: vec!["auto-capture".into(), "file-change".into()],
            file_path: Some(file_path.to_string()),
            event_type: "tool_use".into(),
        }
    } else {
        CaptureIntent::Save {
            kind: MemoryKind::Decision,
            title,
            content,
            importance: 0.4,
            tags: vec!["auto-capture".into(), "file-change".into()],
        }
    }
}

/// Bash command — only capture if the output looks like an error.
fn classify_bash_output(event: &HookEvent, session_compression: bool) -> CaptureIntent {
    let output = event.tool_output.as_deref().unwrap_or("");
    let command = event
        .tool_input
        .as_ref()
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown command");

    // Heuristic: check for error indicators in output
    let is_error = output.contains("error")
        || output.contains("Error")
        || output.contains("ERROR")
        || output.contains("fatal:")
        || output.contains("panic")
        || output.contains("FAILED")
        || output.contains("command not found")
        || output.contains("No such file or directory")
        || output.contains("Permission denied");

    if !is_error {
        return CaptureIntent::Skip {
            reason: "Bash output does not look like an error".into(),
        };
    }

    let cmd_preview = truncate(command, 200);
    let out_preview = truncate(output, 500);
    let title = format!("Bash error: {}", truncate(command, 60));
    let content = format!("Command:\n```\n{cmd_preview}\n```\n\nOutput:\n```\n{out_preview}\n```");

    if session_compression {
        CaptureIntent::Buffer {
            kind: MemoryKind::Error,
            title,
            content,
            importance: 0.6,
            tags: vec!["auto-capture".into(), "bash-error".into()],
            file_path: None,
            event_type: "tool_use".into(),
        }
    } else {
        CaptureIntent::Save {
            kind: MemoryKind::Error,
            title,
            content,
            importance: 0.6,
            tags: vec!["auto-capture".into(), "bash-error".into()],
        }
    }
}

/// PostToolUseFailure — always capture tool failures.
fn classify_failure(event: &HookEvent, session_compression: bool) -> CaptureIntent {
    let tool = event.tool_name.as_deref().unwrap_or("unknown");
    let error = event.error.as_deref().unwrap_or("unknown error");
    let err_preview = truncate(error, 500);
    let title = format!("Tool failure: {tool}");
    let content = format!("Tool `{tool}` failed:\n\n{err_preview}");

    if session_compression {
        CaptureIntent::Buffer {
            kind: MemoryKind::Error,
            title,
            content,
            importance: 0.7,
            tags: vec!["auto-capture".into(), "tool-failure".into()],
            file_path: None,
            event_type: "tool_failure".into(),
        }
    } else {
        CaptureIntent::Save {
            kind: MemoryKind::Error,
            title,
            content,
            importance: 0.7,
            tags: vec!["auto-capture".into(), "tool-failure".into()],
        }
    }
}

/// UserPromptSubmit — capture user intent for session compression context.
/// Not saved as a standalone memory; buffered for compression.
fn classify_user_prompt(event: &HookEvent) -> CaptureIntent {
    let prompt = event.prompt.as_deref().unwrap_or("");
    if prompt.len() < 10 {
        return CaptureIntent::Skip {
            reason: "prompt too short".into(),
        };
    }

    CaptureIntent::Buffer {
        kind: MemoryKind::Observation,
        title: "User intent".into(),
        content: truncate(prompt, 500),
        importance: 0.3,
        tags: Vec::new(),
        file_path: None,
        event_type: "intent".into(),
    }
}

/// Truncate a string to at most `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid char boundary
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Extract the last path component (basename) from a path string.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(hook_event_name: &str) -> HookEvent {
        HookEvent {
            session_id: "test-session".into(),
            cwd: "/home/user/project".into(),
            hook_event_name: hook_event_name.into(),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            error: None,
            stop_hook_active: None,
            prompt: None,
        }
    }

    // -- Tests with session_compression = false (legacy behavior) --

    #[test]
    fn test_skip_unhandled_event() {
        let event = make_event("PreToolUse");
        match classify(&event, false) {
            CaptureIntent::Skip { reason } => assert!(reason.contains("unhandled")),
            _ => panic!("expected Skip"),
        }
    }

    #[test]
    fn test_classify_edit_no_compression() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Edit".into());
        event.tool_input = Some(serde_json::json!({
            "file_path": "/src/main.rs",
            "old_string": "fn old()",
            "new_string": "fn new()"
        }));

        match classify(&event, false) {
            CaptureIntent::Save {
                kind,
                title,
                importance,
                ..
            } => {
                assert_eq!(kind, MemoryKind::Decision);
                assert!(title.contains("main.rs"));
                assert!(title.starts_with("Edit "));
                assert!((importance - 0.4).abs() < f32::EPSILON);
            }
            _ => panic!("expected Save"),
        }
    }

    #[test]
    fn test_classify_bash_success_skipped() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Bash".into());
        event.tool_input = Some(serde_json::json!({ "command": "ls" }));
        event.tool_output = Some("file1.rs\nfile2.rs".into());

        match classify(&event, false) {
            CaptureIntent::Skip { .. } => {}
            _ => panic!("expected Skip for successful bash"),
        }
    }

    #[test]
    fn test_classify_bash_error_captured() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Bash".into());
        event.tool_input = Some(serde_json::json!({ "command": "cargo build" }));
        event.tool_output = Some("error[E0308]: mismatched types".into());

        match classify(&event, false) {
            CaptureIntent::Save {
                kind, importance, ..
            } => {
                assert_eq!(kind, MemoryKind::Error);
                assert!((importance - 0.6).abs() < f32::EPSILON);
            }
            _ => panic!("expected Save for bash error"),
        }
    }

    #[test]
    fn test_classify_tool_failure() {
        let mut event = make_event("PostToolUseFailure");
        event.tool_name = Some("Bash".into());
        event.error = Some("command not found: foo".into());

        match classify(&event, false) {
            CaptureIntent::Save {
                kind, importance, ..
            } => {
                assert_eq!(kind, MemoryKind::Error);
                assert!((importance - 0.7).abs() < f32::EPSILON);
            }
            _ => panic!("expected Save for tool failure"),
        }
    }

    #[test]
    fn test_classify_stop_returns_skip() {
        let event = make_event("Stop");
        match classify(&event, false) {
            CaptureIntent::Skip { .. } => {}
            _ => panic!("expected Skip for Stop (handled separately)"),
        }
    }

    // -- Tests with session_compression = true --

    #[test]
    fn test_classify_edit_with_compression_buffers() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Edit".into());
        event.tool_input = Some(serde_json::json!({
            "file_path": "/src/main.rs",
            "old_string": "fn old()",
            "new_string": "fn new()"
        }));

        match classify(&event, true) {
            CaptureIntent::Buffer {
                kind,
                title,
                file_path,
                event_type,
                ..
            } => {
                assert_eq!(kind, MemoryKind::Decision);
                assert!(title.contains("main.rs"));
                assert_eq!(file_path, Some("/src/main.rs".to_string()));
                assert_eq!(event_type, "tool_use");
            }
            _ => panic!("expected Buffer"),
        }
    }

    #[test]
    fn test_classify_bash_error_with_compression_buffers() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Bash".into());
        event.tool_input = Some(serde_json::json!({ "command": "cargo build" }));
        event.tool_output = Some("error[E0308]: mismatched types".into());

        match classify(&event, true) {
            CaptureIntent::Buffer {
                kind, event_type, ..
            } => {
                assert_eq!(kind, MemoryKind::Error);
                assert_eq!(event_type, "tool_use");
            }
            _ => panic!("expected Buffer for bash error with compression"),
        }
    }

    #[test]
    fn test_classify_failure_with_compression_buffers() {
        let mut event = make_event("PostToolUseFailure");
        event.tool_name = Some("Bash".into());
        event.error = Some("command not found: foo".into());

        match classify(&event, true) {
            CaptureIntent::Buffer {
                kind, event_type, ..
            } => {
                assert_eq!(kind, MemoryKind::Error);
                assert_eq!(event_type, "tool_failure");
            }
            _ => panic!("expected Buffer for tool failure with compression"),
        }
    }

    // -- UserPromptSubmit tests --

    #[test]
    fn test_classify_user_prompt_buffers() {
        let mut event = make_event("UserPromptSubmit");
        event.prompt = Some("Fix the authentication bug in the login flow".into());

        match classify(&event, true) {
            CaptureIntent::Buffer {
                event_type,
                content,
                ..
            } => {
                assert_eq!(event_type, "intent");
                assert!(content.contains("authentication bug"));
            }
            _ => panic!("expected Buffer for UserPromptSubmit"),
        }
    }

    #[test]
    fn test_classify_user_prompt_too_short() {
        let mut event = make_event("UserPromptSubmit");
        event.prompt = Some("hi".into());

        match classify(&event, true) {
            CaptureIntent::Skip { reason } => assert!(reason.contains("too short")),
            _ => panic!("expected Skip for short prompt"),
        }
    }

    #[test]
    fn test_classify_user_prompt_no_prompt() {
        let event = make_event("UserPromptSubmit");
        match classify(&event, false) {
            CaptureIntent::Skip { reason } => assert!(reason.contains("too short")),
            _ => panic!("expected Skip for missing prompt"),
        }
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "a".repeat(300);
        let result = truncate(&long, 200);
        assert_eq!(result.len(), 203); // 200 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_untracked_tool_skipped() {
        let mut event = make_event("PostToolUse");
        event.tool_name = Some("Read".into());

        match classify(&event, false) {
            CaptureIntent::Skip { reason } => assert!(reason.contains("untracked")),
            _ => panic!("expected Skip for Read tool"),
        }
    }
}
