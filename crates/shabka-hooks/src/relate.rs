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
}
