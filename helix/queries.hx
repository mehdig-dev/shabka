// Kaizen HelixDB Queries (v2 HQL)

// -- Memory CRUD --

QUERY save_memory(
    id: String,
    kind: String,
    title: String,
    content: String,
    summary: String,
    tags: String,
    source: String,
    scope: String,
    importance: F64,
    status: String,
    privacy: String,
    project_id: String,
    session_id: String,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String,
    embedding: [F64]
) =>
    memory <- AddN<Memory>({
        memory_id: id,
        kind: kind,
        title: title,
        content: content,
        summary: summary,
        tags: tags,
        source: source,
        scope: scope,
        importance: importance,
        status: status,
        privacy: privacy,
        project_id: project_id,
        session_id: session_id,
        created_by: created_by,
        created_at: created_at,
        updated_at: updated_at,
        accessed_at: accessed_at
    })
    memory_vec <- AddV<MemoryEmbedding>(embedding, {
        memory_id: id,
        title: title
    })
    RETURN memory

// Node-only save (no vector) — used for updates where embedding doesn't change
QUERY save_memory_node(
    id: String,
    kind: String,
    title: String,
    content: String,
    summary: String,
    tags: String,
    source: String,
    scope: String,
    importance: F64,
    status: String,
    privacy: String,
    project_id: String,
    session_id: String,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String
) =>
    memory <- AddN<Memory>({
        memory_id: id,
        kind: kind,
        title: title,
        content: content,
        summary: summary,
        tags: tags,
        source: source,
        scope: scope,
        importance: importance,
        status: status,
        privacy: privacy,
        project_id: project_id,
        session_id: session_id,
        created_by: created_by,
        created_at: created_at,
        updated_at: updated_at,
        accessed_at: accessed_at
    })
    RETURN memory

QUERY get_memory(id: String) =>
    memory <- N<Memory>({memory_id: id})
    RETURN memory

QUERY get_memories(ids: [String]) =>
    memory <- N<Memory>::WHERE(_::{memory_id}::IS_IN(ids))
    RETURN memory

QUERY delete_memory(id: String) =>
    DROP N<Memory>({memory_id: id})
    RETURN NONE

// -- Vector Search --

QUERY search_memories(embedding: [F64], limit: I64) =>
    results <- SearchV<MemoryEmbedding>(embedding, limit)
    RETURN results

// -- Timeline --
// Date filtering done in Rust; HelixDB Value doesn't support String ordering

QUERY timeline(limit: I64) =>
    memory <- N<Memory>::RANGE(0, limit)
    RETURN memory

// -- Graph Relationships --

QUERY add_relation(source_id: String, target_id: String, relation_type: String, strength: F64) =>
    source <- N<Memory>({memory_id: source_id})
    target <- N<Memory>({memory_id: target_id})
    rel <- AddE<RelatesTo>({relation_type: relation_type, strength: strength})::From(source)::To(target)
    RETURN rel

QUERY get_relations(memory_id: String) =>
    source <- N<Memory>({memory_id: memory_id})
    target <- source::Out<RelatesTo>
    RETURN source, target

QUERY get_incoming_relations(memory_id: String) =>
    target <- N<Memory>({memory_id: memory_id})
    source <- target::In<RelatesTo>
    RETURN source, target

// -- Session --

QUERY save_session(id: String, project_id: String, started_at: String, ended_at: String, summary: String, memory_count: I64) =>
    session <- AddN<Session>({
        session_id: id,
        project_id: project_id,
        started_at: started_at,
        ended_at: ended_at,
        summary: summary,
        memory_count: memory_count
    })
    RETURN session

QUERY get_session(id: String) =>
    session <- N<Session>({session_id: id})
    RETURN session
