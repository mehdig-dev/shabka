use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, Json};
use axum::routing::get;
use axum::Router;
use kaizen_core::model::{RelationType, TimelineQuery};
use kaizen_core::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/graph", get(graph_page))
        .route("/graph/data", get(graph_data))
        .route("/api/memories/{id}", get(memory_json))
        .route("/api/memories/{id}/chain", get(memory_chain))
}

#[derive(Template)]
#[template(path = "graph.html")]
struct GraphTemplate;

async fn graph_page() -> Result<Html<String>, AppError> {
    let tmpl = GraphTemplate;
    Ok(Html(tmpl.render()?))
}

// -- JSON API for graph data --

#[derive(Serialize)]
struct GraphData {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

#[derive(Serialize)]
struct GraphNode {
    id: String,
    title: String,
    kind: String,
    importance: f32,
    created_at: String,
}

#[derive(Serialize)]
struct GraphEdge {
    source: String,
    target: String,
    relation_type: String,
    strength: f32,
}

#[derive(Serialize)]
struct MemoryDetail {
    id: String,
    title: String,
    kind: String,
    content: String,
    importance: f32,
    status: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

async fn graph_data(State(state): State<Arc<AppState>>) -> Result<Json<GraphData>, AppError> {
    let query = TimelineQuery {
        limit: 2000,
        ..Default::default()
    };
    let entries = state.storage.timeline(&query).await?;

    let nodes: Vec<GraphNode> = entries
        .iter()
        .map(|e| GraphNode {
            id: e.id.to_string(),
            title: e.title.clone(),
            kind: e.kind.to_string(),
            importance: e.importance,
            created_at: e.created_at.format("%Y-%m-%d %H:%M").to_string(),
        })
        .collect();

    let mut edges = Vec::new();
    let mut seen_pairs = std::collections::HashSet::new();

    for entry in &entries {
        let relations = state
            .storage
            .get_relations(entry.id)
            .await
            .unwrap_or_default();
        for rel in relations {
            let pair = if rel.source_id < rel.target_id {
                (rel.source_id, rel.target_id)
            } else {
                (rel.target_id, rel.source_id)
            };
            if seen_pairs.insert(pair) {
                edges.push(GraphEdge {
                    source: rel.source_id.to_string(),
                    target: rel.target_id.to_string(),
                    relation_type: rel.relation_type.to_string(),
                    strength: rel.strength,
                });
            }
        }
    }

    Ok(Json(GraphData { nodes, edges }))
}

async fn memory_json(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryDetail>, AppError> {
    let memory = state.storage.get_memory(id).await?;
    Ok(Json(MemoryDetail {
        id: memory.id.to_string(),
        title: memory.title,
        kind: memory.kind.to_string(),
        content: memory.content,
        importance: memory.importance,
        status: memory.status.to_string(),
        tags: memory.tags,
        created_at: memory.created_at.format("%Y-%m-%d %H:%M").to_string(),
        updated_at: memory.updated_at.format("%Y-%m-%d %H:%M").to_string(),
    }))
}

// -- Chain API --

#[derive(Deserialize)]
struct ChainQueryParams {
    depth: Option<usize>,
    relation: Option<String>,
}

#[derive(Serialize)]
struct ChainData {
    center: GraphNode,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

async fn memory_chain(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<ChainQueryParams>,
) -> Result<Json<ChainData>, AppError> {
    let depth = params.depth.unwrap_or(3).min(5);

    // Parse relation types from comma-separated string, default to all
    let relation_types: Vec<RelationType> = if let Some(ref rel_str) = params.relation {
        rel_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    } else {
        vec![
            RelationType::CausedBy,
            RelationType::Fixes,
            RelationType::Supersedes,
            RelationType::Related,
            RelationType::Contradicts,
        ]
    };

    let chain_links =
        kaizen_core::graph::follow_chain(&state.storage, id, &relation_types, Some(depth)).await;

    // Collect all memory IDs we need to fetch (center + chain neighbors)
    let mut all_ids: Vec<Uuid> = chain_links.iter().map(|l| l.memory_id).collect();
    all_ids.push(id);
    all_ids.dedup();

    let memories = state.storage.get_memories(&all_ids).await?;

    let center_mem = memories
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow::anyhow!("memory not found"))?;

    let center = GraphNode {
        id: center_mem.id.to_string(),
        title: center_mem.title.clone(),
        kind: center_mem.kind.to_string(),
        importance: center_mem.importance,
        created_at: center_mem.created_at.format("%Y-%m-%d %H:%M").to_string(),
    };

    let nodes: Vec<GraphNode> = chain_links
        .iter()
        .filter_map(|link| {
            memories
                .iter()
                .find(|m| m.id == link.memory_id)
                .map(|m| GraphNode {
                    id: m.id.to_string(),
                    title: m.title.clone(),
                    kind: m.kind.to_string(),
                    importance: m.importance,
                    created_at: m.created_at.format("%Y-%m-%d %H:%M").to_string(),
                })
        })
        .collect();

    let edges: Vec<GraphEdge> = chain_links
        .iter()
        .map(|link| GraphEdge {
            source: link.from_id.to_string(),
            target: link.memory_id.to_string(),
            relation_type: link.relation_type.to_string(),
            strength: link.strength,
        })
        .collect();

    Ok(Json(ChainData {
        center,
        nodes,
        edges,
    }))
}
