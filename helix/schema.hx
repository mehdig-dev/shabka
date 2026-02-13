// Kaizen HelixDB Schema (v2 HQL)
// Nodes, Edges, and Vector Indexes

// -- Nodes --

N::Memory {
    INDEX memory_id: String,
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
    INDEX project_id: String,
    INDEX session_id: String,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String
}

N::Session {
    INDEX session_id: String,
    project_id: String,
    started_at: String,
    ended_at: String,
    summary: String,
    memory_count: I64
}

// -- Edges --

E::RelatesTo {
    From: Memory,
    To: Memory,
    Properties: {
        relation_type: String,
        strength: F64
    }
}

// -- Vector Index --

V::MemoryEmbedding {
    memory_id: String,
    title: String
}
