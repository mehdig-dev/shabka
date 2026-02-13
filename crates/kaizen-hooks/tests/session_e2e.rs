//! End-to-end tests for session compression via the kaizen-hooks binary.
//!
//! Requires HelixDB at localhost:6969 (`just db`) and Ollama with nomic-embed-text.
//! These tests invoke the hooks binary as a subprocess, piping hook events via stdin.
//!
//! Run: `cargo test -p kaizen-hooks --no-default-features --test session_e2e -- --ignored`

use std::io::Write;
use std::process::{Command, Stdio};

/// Check if HelixDB is reachable.
fn helix_available() -> bool {
    let output = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-X", "POST"])
        .args(["-H", "Content-Type: application/json"])
        .args(["-d", "{\"limit\": 1}"])
        .arg("http://localhost:6969/timeline")
        .output();

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("200"),
        Err(_) => false,
    }
}

/// Get the path to the hooks binary.
fn hooks_bin() -> String {
    let mut path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    path.push("kaizen-hooks");
    path.to_string_lossy().to_string()
}

/// Send a hook event to the hooks binary via stdin.
fn send_hook_event(json: &str) -> (String, String, i32) {
    let mut child = Command::new(hooks_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn kaizen-hooks");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().expect("failed to wait for hooks");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Generate a unique session ID for test isolation.
fn test_session_id() -> String {
    format!("test-session-{}", uuid::Uuid::now_v7())
}

/// PostToolUse event for a file edit.
fn edit_event(session_id: &str, tool_name: &str, file_path: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "cwd": "/tmp",
        "hook_event_name": "PostToolUse",
        "tool_name": tool_name,
        "tool_input": {
            "file_path": file_path,
            "old_string": "old code",
            "new_string": "new code"
        },
        "tool_output": "File edited successfully"
    })
    .to_string()
}

/// Stop event to trigger session compression.
fn stop_event(session_id: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "cwd": "/tmp",
        "hook_event_name": "Stop"
    })
    .to_string()
}

/// UserPromptSubmit event for intent capture.
fn prompt_event(session_id: &str, prompt: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "cwd": "/tmp",
        "hook_event_name": "UserPromptSubmit",
        "prompt": prompt
    })
    .to_string()
}

/// Test: PostToolUse events should always exit 0 (never block Claude Code).
#[test]
#[ignore]
fn test_hooks_always_exit_zero() {
    let session = test_session_id();
    let event = edit_event(&session, "Edit", "/tmp/test.rs");

    let (_, _, code) = send_hook_event(&event);
    assert_eq!(code, 0, "hooks should always exit 0");

    // Cleanup: send stop to flush buffer
    send_hook_event(&stop_event(&session));
}

/// Test: Malformed JSON input should exit 0 (not crash).
#[test]
#[ignore]
fn test_hooks_malformed_input() {
    let (_, _, code) = send_hook_event("not json at all");
    assert_eq!(code, 0, "hooks should handle malformed input gracefully");
}

/// Test: Empty input should exit 0.
#[test]
#[ignore]
fn test_hooks_empty_input() {
    let (_, _, code) = send_hook_event("");
    assert_eq!(code, 0, "hooks should handle empty input gracefully");
}

/// Test: Edit events create a session buffer file.
#[test]
#[ignore]
fn test_hooks_creates_session_buffer() {
    let session = test_session_id();

    // Send a few edit events
    send_hook_event(&edit_event(&session, "Edit", "/tmp/foo.rs"));
    send_hook_event(&edit_event(&session, "Write", "/tmp/bar.rs"));

    // Check buffer file exists
    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let buffer_path = config_dir
        .join("kaizen")
        .join("sessions")
        .join(format!("{session}.jsonl"));

    assert!(
        buffer_path.exists(),
        "session buffer should exist at {}",
        buffer_path.display()
    );

    // Read and verify content
    let content = std::fs::read_to_string(&buffer_path).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "should have 2 buffered events");

    // Each line should be valid JSON
    for line in &lines {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "buffer line should be valid JSON: {line}");
    }

    // Cleanup: send stop + delete buffer
    send_hook_event(&stop_event(&session));
    let _ = std::fs::remove_file(&buffer_path);
}

/// Test: Stop event triggers compression and removes buffer.
#[test]
#[ignore]
fn test_hooks_stop_compresses_and_cleans_buffer() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let session = test_session_id();

    // Buffer some events
    send_hook_event(&edit_event(&session, "Edit", "/tmp/test_compress.rs"));
    send_hook_event(&edit_event(&session, "Edit", "/tmp/test_compress.rs"));
    send_hook_event(&edit_event(&session, "Write", "/tmp/test_new.rs"));

    // Verify buffer exists
    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let buffer_path = config_dir
        .join("kaizen")
        .join("sessions")
        .join(format!("{session}.jsonl"));

    assert!(buffer_path.exists(), "buffer should exist before stop");

    // Send stop — triggers compression
    let (_, stderr, code) = send_hook_event(&stop_event(&session));
    assert_eq!(code, 0, "stop should exit 0");

    // Buffer should be cleaned up after stop
    assert!(
        !buffer_path.exists(),
        "buffer should be deleted after stop: stderr={stderr}"
    );
}

/// Test: UserPromptSubmit events are buffered as intent context.
#[test]
#[ignore]
fn test_hooks_user_prompt_buffered() {
    let session = test_session_id();

    // Send a user prompt event
    send_hook_event(&prompt_event(&session, "Fix the authentication middleware"));

    // Check buffer
    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let buffer_path = config_dir
        .join("kaizen")
        .join("sessions")
        .join(format!("{session}.jsonl"));

    assert!(buffer_path.exists(), "prompt should be buffered");

    let content = std::fs::read_to_string(&buffer_path).unwrap();
    assert!(
        content.contains("intent"),
        "buffered event should have intent event_type"
    );
    assert!(
        content.contains("authentication"),
        "buffered event should contain prompt text"
    );

    // Cleanup
    send_hook_event(&stop_event(&session));
    let _ = std::fs::remove_file(&buffer_path);
}

/// Test: Short prompts are skipped (not buffered).
#[test]
#[ignore]
fn test_hooks_short_prompt_skipped() {
    let session = test_session_id();

    // Short prompt should be skipped
    send_hook_event(&prompt_event(&session, "yes"));

    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let buffer_path = config_dir
        .join("kaizen")
        .join("sessions")
        .join(format!("{session}.jsonl"));

    // Buffer should not exist (or be empty) for a short prompt
    if buffer_path.exists() {
        let content = std::fs::read_to_string(&buffer_path).unwrap();
        assert!(
            content.trim().is_empty(),
            "short prompt should not be buffered: {content}"
        );
        let _ = std::fs::remove_file(&buffer_path);
    }
}

/// Test: Untracked tools (like Read) are skipped, not buffered.
#[test]
#[ignore]
fn test_hooks_untracked_tool_skipped() {
    let session = test_session_id();

    let event = serde_json::json!({
        "session_id": session,
        "cwd": "/tmp",
        "hook_event_name": "PostToolUse",
        "tool_name": "Read",
        "tool_input": {"file_path": "/tmp/foo.rs"},
        "tool_output": "file contents"
    })
    .to_string();

    send_hook_event(&event);

    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let buffer_path = config_dir
        .join("kaizen")
        .join("sessions")
        .join(format!("{session}.jsonl"));

    // Read events should not be buffered
    if buffer_path.exists() {
        let content = std::fs::read_to_string(&buffer_path).unwrap();
        assert!(
            content.trim().is_empty(),
            "Read tool should not be buffered: {content}"
        );
        let _ = std::fs::remove_file(&buffer_path);
    }
}
