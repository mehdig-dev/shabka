use shabka_core::model::*;
use shabka_core::storage::{Storage, StorageBackend};

/// Auto-create relations for a newly saved memory.
///
/// Runs three strategies:
/// 1. Session thread — link to previous memory from the same session
/// 2. Same-file clustering — link file-change memories touching the same file
/// 3. Error→Fix chains — link edits that follow recent errors mentioning the same file
pub async fn auto_relate(storage: &Storage, memory: &Memory, session_id: &str) {
    // Fetch recent memories to match against
    let query = TimelineQuery {
        limit: 50,
        ..Default::default()
    };
    let recent = match storage.timeline(&query).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!("auto_relate: failed to fetch timeline: {e}");
            return;
        }
    };

    // Fetch full memories for the candidates we'll actually compare against
    let candidate_ids: Vec<_> = recent
        .iter()
        .filter(|e| e.id != memory.id)
        .map(|e| e.id)
        .collect();

    if candidate_ids.is_empty() {
        return;
    }

    let candidates = match storage.get_memories(&candidate_ids).await {
        Ok(mems) => mems,
        Err(e) => {
            tracing::debug!("auto_relate: failed to fetch candidates: {e}");
            return;
        }
    };

    let file_path = extract_file_path(&memory.content);

    // Strategy 1: Session thread
    session_thread(storage, memory, session_id, &candidates).await;

    // Strategy 2: Same-file clustering
    if let Some(ref path) = file_path {
        same_file_cluster(storage, memory, path, &candidates).await;
    }

    // Strategy 3: Error → Fix chains
    if memory.kind == MemoryKind::Decision {
        error_fix_chain(storage, memory, file_path.as_deref(), &candidates).await;
    }
}

/// Link to the most recent memory from the same session (sequential chain).
async fn session_thread(
    storage: &Storage,
    memory: &Memory,
    session_id: &str,
    candidates: &[Memory],
) {
    if session_id.is_empty() {
        return;
    }

    // Find most recent candidate from the same session
    // Candidates are ordered by created_at desc from timeline
    let prev = candidates.iter().find(|c| {
        c.created_by == "shabka-hooks" && c.content.contains(session_id)
            || c.session_id.map(|s| s.to_string()) == Some(session_id.to_string())
    });

    // Fallback: find any memory tagged auto-capture that was created recently
    // (within last 10 candidates) since session_id might not be stored on the memory
    let prev = prev.or_else(|| {
        candidates.iter().take(10).find(|c| {
            c.created_by == "shabka-hooks" && c.tags.contains(&"auto-capture".to_string())
        })
    });

    if let Some(prev) = prev {
        let rel = MemoryRelation {
            source_id: prev.id,
            target_id: memory.id,
            relation_type: RelationType::Related,
            strength: 0.4,
        };
        if let Err(e) = storage.add_relation(&rel).await {
            tracing::debug!("session_thread: failed to add relation: {e}");
        }
    }
}

/// Link file-change memories that touch the same file.
async fn same_file_cluster(
    storage: &Storage,
    memory: &Memory,
    file_path: &str,
    candidates: &[Memory],
) {
    let filename = basename(file_path);

    // Find recent memories referencing the same file (by filename match in content)
    let same_file: Vec<&Memory> = candidates
        .iter()
        .filter(|c| {
            c.kind == MemoryKind::Decision && c.id != memory.id && c.content.contains(filename)
        })
        .take(3) // Link to at most 3 recent edits to the same file
        .collect();

    for prev in same_file {
        let rel = MemoryRelation {
            source_id: prev.id,
            target_id: memory.id,
            relation_type: RelationType::Related,
            strength: 0.6,
        };
        if let Err(e) = storage.add_relation(&rel).await {
            tracing::debug!("same_file_cluster: failed to add relation: {e}");
        }
    }
}

/// Link an edit (decision) to a recent error it likely fixes.
///
/// Heuristic: if a recent Error memory's content mentions the same file
/// or filename that was just edited, the edit probably fixes the error.
async fn error_fix_chain(
    storage: &Storage,
    memory: &Memory,
    file_path: Option<&str>,
    candidates: &[Memory],
) {
    let filename = file_path.map(basename).unwrap_or("");
    if filename.is_empty() {
        return;
    }

    // Look at last 15 candidates for recent errors
    let recent_errors: Vec<&Memory> = candidates
        .iter()
        .take(15)
        .filter(|c| {
            c.kind == MemoryKind::Error
                && (c.content.contains(filename) || c.title.contains(filename))
        })
        .take(2)
        .collect();

    for error in recent_errors {
        let rel = MemoryRelation {
            source_id: memory.id,
            target_id: error.id,
            relation_type: RelationType::Fixes,
            strength: 0.7,
        };
        if let Err(e) = storage.add_relation(&rel).await {
            tracing::debug!("error_fix_chain: failed to add relation: {e}");
        }
    }
}

/// Extract a file path from memory content.
/// Looks for "File modified via Edit: /path/to/file" pattern.
fn extract_file_path(content: &str) -> Option<String> {
    let prefix = "File modified via ";
    let line = content.lines().find(|l| l.starts_with(prefix))?;
    // "File modified via Edit: /path/to/file"
    let after_colon = line.split_once(": ")?.1;
    // Take everything after "Edit: " or "Write: "
    let path = after_colon
        .split_once(": ")
        .map(|(_, p)| p)
        .unwrap_or(after_colon);
    if path.starts_with('/') {
        Some(path.to_string())
    } else {
        None
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_file_path_edit() {
        let content = "File modified via Edit: /home/user/project/src/main.rs\n\nReplaced:";
        assert_eq!(
            extract_file_path(content),
            Some("/home/user/project/src/main.rs".to_string())
        );
    }

    #[test]
    fn test_extract_file_path_write() {
        let content = "File modified via Write: /home/user/project/new_file.rs";
        assert_eq!(
            extract_file_path(content),
            Some("/home/user/project/new_file.rs".to_string())
        );
    }

    #[test]
    fn test_extract_file_path_none() {
        let content = "Tool `Bash` failed:\n\nExit code 1";
        assert_eq!(extract_file_path(content), None);
    }

    #[test]
    fn test_basename() {
        assert_eq!(basename("/home/user/project/src/main.rs"), "main.rs");
        assert_eq!(basename("main.rs"), "main.rs");
    }

    #[test]
    fn test_basename_extraction() {
        assert_eq!(basename("src/auth.rs"), "auth.rs");
        assert_eq!(basename("/a/b/c/foo.txt"), "foo.txt");
        assert_eq!(basename("no_slash"), "no_slash");
        assert_eq!(basename(""), "");
    }

    // -- Async tests requiring storage --

    use shabka_core::storage::SqliteStorage;

    fn make_memory(title: &str, content: &str, kind: MemoryKind, created_by: &str) -> Memory {
        Memory::new(
            title.to_string(),
            content.to_string(),
            kind,
            created_by.to_string(),
        )
    }

    fn test_storage() -> Storage {
        Storage::Sqlite(SqliteStorage::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_session_thread_links_same_session() {
        let storage = test_storage();
        let session_id = "sess-123";

        // Save an older memory that mentions the session and is from shabka-hooks
        let mut older = make_memory(
            "Previous memory",
            &format!("Some work done session={session_id}"),
            MemoryKind::Decision,
            "shabka-hooks",
        );
        older.tags = vec!["auto-capture".to_string()];
        storage.save_memory(&older, None).await.unwrap();

        // Create a newer memory
        let newer = make_memory(
            "New memory",
            "New work done",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&newer, None).await.unwrap();

        // Call session_thread with the newer memory
        let candidates = vec![older.clone()];
        session_thread(&storage, &newer, session_id, &candidates).await;

        // Verify relation was created (check from the older memory side)
        let rels = storage.get_relations(older.id).await.unwrap();
        assert!(
            !rels.is_empty(),
            "session_thread should create a relation from older to newer"
        );
        assert_eq!(rels[0].relation_type, RelationType::Related);
    }

    #[tokio::test]
    async fn test_session_thread_skips_empty_session() {
        let storage = test_storage();

        let mem = make_memory(
            "Some memory",
            "content",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&mem, None).await.unwrap();

        let candidates = vec![mem.clone()];
        let newer = make_memory("Newer", "content2", MemoryKind::Decision, "shabka-hooks");
        storage.save_memory(&newer, None).await.unwrap();

        // Call with empty session_id — should be a no-op
        session_thread(&storage, &newer, "", &candidates).await;

        let rels = storage.get_relations(mem.id).await.unwrap();
        assert!(
            rels.is_empty(),
            "session_thread with empty session_id should not create relations"
        );
    }

    #[tokio::test]
    async fn test_same_file_cluster_links_edits() {
        let storage = test_storage();

        // Save a Decision memory whose content mentions auth.rs
        let older = make_memory(
            "Edit auth.rs",
            "File modified via Edit: /src/auth.rs\n\nReplaced old code",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&older, None).await.unwrap();

        // Create a newer memory editing the same file
        let newer = make_memory(
            "Edit auth.rs again",
            "File modified via Edit: /src/auth.rs\n\nAdded new function",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&newer, None).await.unwrap();

        let candidates = vec![older.clone()];
        same_file_cluster(&storage, &newer, "/src/auth.rs", &candidates).await;

        let rels = storage.get_relations(older.id).await.unwrap();
        assert!(
            !rels.is_empty(),
            "same_file_cluster should create a relation for same-file edits"
        );
        assert_eq!(rels[0].relation_type, RelationType::Related);
    }

    #[tokio::test]
    async fn test_error_fix_chain_links_error_to_fix() {
        let storage = test_storage();

        // Save an Error memory mentioning auth.rs
        let error_mem = make_memory(
            "Build failed in auth.rs",
            "error[E0308]: mismatched types in auth.rs",
            MemoryKind::Error,
            "shabka-hooks",
        );
        storage.save_memory(&error_mem, None).await.unwrap();

        // Create a Decision (fix) memory for the same file
        let fix_mem = make_memory(
            "Fix auth.rs types",
            "File modified via Edit: /src/auth.rs\n\nFixed type mismatch",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&fix_mem, None).await.unwrap();

        let candidates = vec![error_mem.clone()];
        error_fix_chain(&storage, &fix_mem, Some("/src/auth.rs"), &candidates).await;

        // The relation goes from fix_mem -> error_mem with type Fixes
        let rels = storage.get_relations(fix_mem.id).await.unwrap();
        assert!(
            !rels.is_empty(),
            "error_fix_chain should create a Fixes relation"
        );
        assert_eq!(rels[0].relation_type, RelationType::Fixes);
        assert_eq!(rels[0].target_id, error_mem.id);
    }

    #[tokio::test]
    async fn test_error_fix_chain_skips_no_file() {
        let storage = test_storage();

        let error_mem = make_memory(
            "Some error",
            "generic error output",
            MemoryKind::Error,
            "shabka-hooks",
        );
        storage.save_memory(&error_mem, None).await.unwrap();

        let fix_mem = make_memory(
            "Fix something",
            "fixed it",
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&fix_mem, None).await.unwrap();

        let candidates = vec![error_mem.clone()];
        // Call with None file_path — should return early
        error_fix_chain(&storage, &fix_mem, None, &candidates).await;

        let rels = storage.get_relations(fix_mem.id).await.unwrap();
        assert!(
            rels.is_empty(),
            "error_fix_chain with no file_path should not create relations"
        );
    }

    #[tokio::test]
    async fn test_auto_relate_runs_all_strategies() {
        let storage = test_storage();
        let session_id = "sess-456";

        // Save an Error memory mentioning config.rs
        let mut error_mem = make_memory(
            "Build error in config.rs",
            "error: cannot find value `cfg` in config.rs",
            MemoryKind::Error,
            "shabka-hooks",
        );
        error_mem.tags = vec!["auto-capture".to_string()];
        storage.save_memory(&error_mem, None).await.unwrap();

        // Save a fix (Decision) memory for the same file, same session
        let fix_mem = make_memory(
            "Fix config.rs",
            &format!(
                "File modified via Edit: /src/config.rs\n\nFixed missing value session={session_id}"
            ),
            MemoryKind::Decision,
            "shabka-hooks",
        );
        storage.save_memory(&fix_mem, None).await.unwrap();

        // auto_relate should run all three strategies
        auto_relate(&storage, &fix_mem, session_id).await;

        // Check that at least one relation exists (error_fix_chain or session_thread)
        let rels_fix = storage.get_relations(fix_mem.id).await.unwrap();
        let rels_err = storage.get_relations(error_mem.id).await.unwrap();
        let total = rels_fix.len() + rels_err.len();
        assert!(
            total >= 1,
            "auto_relate should create at least one relation, got {total}"
        );
    }
}
