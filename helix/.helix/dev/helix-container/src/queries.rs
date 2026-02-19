
// DEFAULT CODE
// use helix_db::helix_engine::traversal_core::config::Config;

// pub fn config() -> Option<Config> {
//     None
// }



use bumpalo::Bump;
use heed3::RoTxn;
use helix_macros::{handler, tool_call, mcp_handler, migration};
use helix_db::{
    helix_engine::{
        reranker::{
            RerankAdapter,
            fusion::{RRFReranker, MMRReranker, DistanceMethod},
        },
        traversal_core::{
            config::{Config, GraphConfig, VectorConfig},
            ops::{
                bm25::search_bm25::SearchBM25Adapter,
                g::G,
                in_::{in_::InAdapter, in_e::InEdgesAdapter, to_n::ToNAdapter, to_v::ToVAdapter},
                out::{
                    from_n::FromNAdapter, from_v::FromVAdapter, out::OutAdapter, out_e::OutEdgesAdapter,
                },
                source::{
                    add_e::AddEAdapter,
                    add_n::AddNAdapter,
                    e_from_id::EFromIdAdapter,
                    e_from_type::EFromTypeAdapter,
                    n_from_id::NFromIdAdapter,
                    n_from_index::NFromIndexAdapter,
                    n_from_type::NFromTypeAdapter,
                    v_from_id::VFromIdAdapter,
                    v_from_type::VFromTypeAdapter
                },
                util::{
                    dedup::DedupAdapter, drop::Drop, exist::Exist, filter_mut::FilterMut,
                    filter_ref::FilterRefAdapter, map::MapAdapter, paths::{PathAlgorithm, ShortestPathAdapter},
                    range::RangeAdapter, update::UpdateAdapter, order::OrderByAdapter,
                    aggregate::AggregateAdapter, group_by::GroupByAdapter, count::CountAdapter,
                    upsert::UpsertAdapter,
                },
                vectors::{
                    brute_force_search::BruteForceSearchVAdapter, insert::InsertVAdapter,
                    search::SearchVAdapter,
                },
            },
            traversal_value::TraversalValue,
        },
        types::{GraphError, SecondaryIndex},
        vector_core::vector::HVector,
    },
    helix_gateway::{
        embedding_providers::{EmbeddingModel, get_embedding_model},
        router::router::{HandlerInput, IoContFn},
        mcp::mcp::{MCPHandlerSubmission, MCPToolInput, MCPHandler}
    },
    node_matches, props, embed, embed_async,
    field_addition_from_old_field, field_type_cast, field_addition_from_value,
    protocol::{
        response::Response,
        value::{casting::{cast, CastType}, Value},
        format::Format,
    },
    utils::{
        id::{ID, uuid_str},
        items::{Edge, Node},
        properties::ImmutablePropertiesMap,
    },
};
use sonic_rs::{Deserialize, Serialize, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use chrono::{DateTime, Utc};

// Re-export scalar types for generated code
type I8 = i8;
type I16 = i16;
type I32 = i32;
type I64 = i64;
type U8 = u8;
type U16 = u16;
type U32 = u32;
type U64 = u64;
type U128 = u128;
type F32 = f32;
type F64 = f64;
    
pub fn config() -> Option<Config> {
return Some(Config {
vector_config: Some(VectorConfig {
m: Some(16),
ef_construction: Some(128),
ef_search: Some(768),
}),
graph_config: Some(GraphConfig {
secondary_indices: Some(vec![SecondaryIndex::Index("memory_id".to_string()), SecondaryIndex::Index("project_id".to_string()), SecondaryIndex::Index("session_id".to_string())]),
}),
db_max_size_gb: Some(20),
mcp: Some(true),
bm25: Some(true),
schema: Some(r#"{
  "schema": {
    "nodes": [
      {
        "name": "Memory",
        "properties": {
          "updated_at": "String",
          "verification": "String",
          "created_at": "String",
          "title": "String",
          "scope": "String",
          "status": "String",
          "tags": "String",
          "privacy": "String",
          "session_id": "String",
          "memory_id": "String",
          "accessed_at": "String",
          "id": "ID",
          "created_by": "String",
          "project_id": "String",
          "label": "String",
          "kind": "String",
          "content": "String",
          "summary": "String",
          "source": "String",
          "importance": "F64"
        }
      },
      {
        "name": "Session",
        "properties": {
          "id": "ID",
          "project_id": "String",
          "summary": "String",
          "ended_at": "String",
          "session_id": "String",
          "label": "String",
          "started_at": "String",
          "memory_count": "I64"
        }
      }
    ],
    "vectors": [
      {
        "name": "MemoryEmbedding",
        "properties": {
          "data": "Array(F64)",
          "title": "String",
          "memory_id": "String",
          "label": "String",
          "id": "ID",
          "score": "F64"
        }
      }
    ],
    "edges": [
      {
        "name": "RelatesTo",
        "from": "Memory",
        "to": "Memory",
        "properties": {
          "relation_type": "String",
          "strength": "F64"
        }
      }
    ]
  },
  "queries": [
    {
      "name": "get_memory",
      "parameters": {
        "id": "String"
      },
      "returns": [
        "memory"
      ]
    },
    {
      "name": "add_relation",
      "parameters": {
        "target_id": "String",
        "relation_type": "String",
        "strength": "F64",
        "source_id": "String"
      },
      "returns": [
        "rel"
      ]
    },
    {
      "name": "get_incoming_relations",
      "parameters": {
        "memory_id": "String"
      },
      "returns": [
        "source",
        "target",
        "edges"
      ]
    },
    {
      "name": "save_session",
      "parameters": {
        "id": "String",
        "started_at": "String",
        "project_id": "String",
        "ended_at": "String",
        "summary": "String",
        "memory_count": "I64"
      },
      "returns": [
        "session"
      ]
    },
    {
      "name": "get_session",
      "parameters": {
        "id": "String"
      },
      "returns": [
        "session"
      ]
    },
    {
      "name": "delete_memory",
      "parameters": {
        "id": "String"
      },
      "returns": []
    },
    {
      "name": "get_relations",
      "parameters": {
        "memory_id": "String"
      },
      "returns": [
        "source",
        "target",
        "edges"
      ]
    },
    {
      "name": "get_memories",
      "parameters": {
        "ids": "Array(String)"
      },
      "returns": [
        "memory"
      ]
    },
    {
      "name": "search_memories",
      "parameters": {
        "embedding": "Array(F64)",
        "limit": "I64"
      },
      "returns": [
        "results"
      ]
    },
    {
      "name": "timeline",
      "parameters": {
        "limit": "I64"
      },
      "returns": [
        "memory"
      ]
    },
    {
      "name": "save_memory",
      "parameters": {
        "project_id": "String",
        "scope": "String",
        "id": "String",
        "summary": "String",
        "privacy": "String",
        "tags": "String",
        "content": "String",
        "source": "String",
        "status": "String",
        "verification": "String",
        "title": "String",
        "created_at": "String",
        "accessed_at": "String",
        "session_id": "String",
        "updated_at": "String",
        "embedding": "Array(F64)",
        "kind": "String",
        "created_by": "String",
        "importance": "F64"
      },
      "returns": [
        "memory"
      ]
    },
    {
      "name": "save_memory_node",
      "parameters": {
        "title": "String",
        "summary": "String",
        "status": "String",
        "privacy": "String",
        "accessed_at": "String",
        "scope": "String",
        "session_id": "String",
        "importance": "F64",
        "updated_at": "String",
        "kind": "String",
        "created_at": "String",
        "content": "String",
        "verification": "String",
        "id": "String",
        "project_id": "String",
        "source": "String",
        "created_by": "String",
        "tags": "String"
      },
      "returns": [
        "memory"
      ]
    }
  ]
}"#.to_string()),
embedding_model: Some("text-embedding-ada-002".to_string()),
graphvis_node_label: None,
})
}
pub struct Memory {
    pub memory_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub summary: String,
    pub tags: String,
    pub source: String,
    pub scope: String,
    pub importance: f64,
    pub status: String,
    pub privacy: String,
    pub project_id: String,
    pub session_id: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub accessed_at: String,
    pub verification: String,
}

pub struct Session {
    pub session_id: String,
    pub project_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub summary: String,
    pub memory_count: i64,
}

pub struct RelatesTo {
    pub from: Memory,
    pub to: Memory,
    pub relation_type: String,
    pub strength: f64,
}

pub struct MemoryEmbedding {
    pub memory_id: String,
    pub title: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct get_memoryInput {

pub id: String
}
#[derive(Serialize, Default)]
pub struct Get_memoryMemoryReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[handler]
pub fn get_memory (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_memoryInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let memory = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.id).collect_to_obj()?;
let response = json!({
    "memory": Get_memoryMemoryReturnType {
        id: uuid_str(memory.id(), &arena),
        label: memory.label(),
        memory_id: memory.get_property("memory_id"),
        kind: memory.get_property("kind"),
        title: memory.get_property("title"),
        content: memory.get_property("content"),
        summary: memory.get_property("summary"),
        tags: memory.get_property("tags"),
        source: memory.get_property("source"),
        scope: memory.get_property("scope"),
        importance: memory.get_property("importance"),
        status: memory.get_property("status"),
        privacy: memory.get_property("privacy"),
        project_id: memory.get_property("project_id"),
        session_id: memory.get_property("session_id"),
        created_by: memory.get_property("created_by"),
        created_at: memory.get_property("created_at"),
        updated_at: memory.get_property("updated_at"),
        accessed_at: memory.get_property("accessed_at"),
        verification: memory.get_property("verification"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct add_relationInput {

pub source_id: String,
pub target_id: String,
pub relation_type: String,
pub strength: f64
}
#[derive(Serialize, Default)]
pub struct Add_relationRelReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub from_node: &'a str,
    pub to_node: &'a str,
    pub relation_type: Option<&'a Value>,
    pub strength: Option<&'a Value>,
}

#[handler(is_write)]
pub fn add_relation (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<add_relationInput>(&input.request.body)?;
let arena = Bump::new();
let mut txn = db.graph_env.write_txn().map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let source = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.source_id).collect_to_obj()?;
    let target = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.target_id).collect_to_obj()?;
    let rel = G::new_mut(&db, &arena, &mut txn)
.add_edge("RelatesTo", Some(ImmutablePropertiesMap::new(2, vec![("strength", Value::from(data.strength.clone())), ("relation_type", Value::from(data.relation_type.clone()))].into_iter(), &arena)), source.id(), target.id(), false, false).collect_to_obj()?;
let response = json!({
    "rel": Add_relationRelReturnType {
        id: uuid_str(rel.id(), &arena),
        label: rel.label(),
        from_node: uuid_str(rel.from_node(), &arena),
        to_node: uuid_str(rel.to_node(), &arena),
        relation_type: rel.get_property("relation_type"),
        strength: rel.get_property("strength"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct get_incoming_relationsInput {

pub memory_id: String
}
#[derive(Serialize, Default)]
pub struct Get_incoming_relationsSourceReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[derive(Serialize, Default)]
pub struct Get_incoming_relationsTargetReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[derive(Serialize, Default)]
pub struct Get_incoming_relationsEdgesReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub from_node: &'a str,
    pub to_node: &'a str,
    pub relation_type: Option<&'a Value>,
    pub strength: Option<&'a Value>,
}

#[handler]
pub fn get_incoming_relations (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_incoming_relationsInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let target = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.memory_id).collect_to_obj()?;
    let edges = G::from_iter(&db, &txn, std::iter::once(target.clone()), &arena)

.in_e("RelatesTo").collect::<Result<Vec<_>, _>>()?;
    let source = G::from_iter(&db, &txn, std::iter::once(target.clone()), &arena)

.in_node("RelatesTo").collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "source": source.iter().map(|source| Get_incoming_relationsSourceReturnType {
        id: uuid_str(source.id(), &arena),
        label: source.label(),
        memory_id: source.get_property("memory_id"),
        kind: source.get_property("kind"),
        title: source.get_property("title"),
        content: source.get_property("content"),
        summary: source.get_property("summary"),
        tags: source.get_property("tags"),
        source: source.get_property("source"),
        scope: source.get_property("scope"),
        importance: source.get_property("importance"),
        status: source.get_property("status"),
        privacy: source.get_property("privacy"),
        project_id: source.get_property("project_id"),
        session_id: source.get_property("session_id"),
        created_by: source.get_property("created_by"),
        created_at: source.get_property("created_at"),
        updated_at: source.get_property("updated_at"),
        accessed_at: source.get_property("accessed_at"),
        verification: source.get_property("verification"),
    }).collect::<Vec<_>>(),
    "target": Get_incoming_relationsTargetReturnType {
        id: uuid_str(target.id(), &arena),
        label: target.label(),
        memory_id: target.get_property("memory_id"),
        kind: target.get_property("kind"),
        title: target.get_property("title"),
        content: target.get_property("content"),
        summary: target.get_property("summary"),
        tags: target.get_property("tags"),
        source: target.get_property("source"),
        scope: target.get_property("scope"),
        importance: target.get_property("importance"),
        status: target.get_property("status"),
        privacy: target.get_property("privacy"),
        project_id: target.get_property("project_id"),
        session_id: target.get_property("session_id"),
        created_by: target.get_property("created_by"),
        created_at: target.get_property("created_at"),
        updated_at: target.get_property("updated_at"),
        accessed_at: target.get_property("accessed_at"),
        verification: target.get_property("verification"),
    },
    "edges": edges.iter().map(|edge| Get_incoming_relationsEdgesReturnType {
        id: uuid_str(edge.id(), &arena),
        label: edge.label(),
        from_node: uuid_str(edge.from_node(), &arena),
        to_node: uuid_str(edge.to_node(), &arena),
        relation_type: edge.get_property("relation_type"),
        strength: edge.get_property("strength"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct save_sessionInput {

pub id: String,
pub project_id: String,
pub started_at: String,
pub ended_at: String,
pub summary: String,
pub memory_count: i64
}
#[derive(Serialize, Default)]
pub struct Save_sessionSessionReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub session_id: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub started_at: Option<&'a Value>,
    pub ended_at: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub memory_count: Option<&'a Value>,
}

#[handler(is_write)]
pub fn save_session (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<save_sessionInput>(&input.request.body)?;
let arena = Bump::new();
let mut txn = db.graph_env.write_txn().map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let session = G::new_mut(&db, &arena, &mut txn)
.add_n("Session", Some(ImmutablePropertiesMap::new(6, vec![("memory_count", Value::from(&data.memory_count)), ("started_at", Value::from(&data.started_at)), ("ended_at", Value::from(&data.ended_at)), ("session_id", Value::from(&data.id)), ("project_id", Value::from(&data.project_id)), ("summary", Value::from(&data.summary))].into_iter(), &arena)), Some(&["session_id"])).collect_to_obj()?;
let response = json!({
    "session": Save_sessionSessionReturnType {
        id: uuid_str(session.id(), &arena),
        label: session.label(),
        session_id: session.get_property("session_id"),
        project_id: session.get_property("project_id"),
        started_at: session.get_property("started_at"),
        ended_at: session.get_property("ended_at"),
        summary: session.get_property("summary"),
        memory_count: session.get_property("memory_count"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct get_sessionInput {

pub id: String
}
#[derive(Serialize, Default)]
pub struct Get_sessionSessionReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub session_id: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub started_at: Option<&'a Value>,
    pub ended_at: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub memory_count: Option<&'a Value>,
}

#[handler]
pub fn get_session (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_sessionInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let session = G::new(&db, &txn, &arena)
.n_from_index("Session", "session_id", &data.id).collect_to_obj()?;
let response = json!({
    "session": Get_sessionSessionReturnType {
        id: uuid_str(session.id(), &arena),
        label: session.label(),
        session_id: session.get_property("session_id"),
        project_id: session.get_property("project_id"),
        started_at: session.get_property("started_at"),
        ended_at: session.get_property("ended_at"),
        summary: session.get_property("summary"),
        memory_count: session.get_property("memory_count"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct delete_memoryInput {

pub id: String
}
#[handler(is_write)]
pub fn delete_memory (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<delete_memoryInput>(&input.request.body)?;
let arena = Bump::new();
let mut txn = db.graph_env.write_txn().map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    Drop::drop_traversal(
                G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.id).collect::<Vec<_>>().into_iter(),
                &db,
                &mut txn,
            )?;;
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&()))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct get_relationsInput {

pub memory_id: String
}
#[derive(Serialize, Default)]
pub struct Get_relationsSourceReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[derive(Serialize, Default)]
pub struct Get_relationsTargetReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[derive(Serialize, Default)]
pub struct Get_relationsEdgesReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub from_node: &'a str,
    pub to_node: &'a str,
    pub relation_type: Option<&'a Value>,
    pub strength: Option<&'a Value>,
}

#[handler]
pub fn get_relations (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_relationsInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let source = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.memory_id).collect_to_obj()?;
    let edges = G::from_iter(&db, &txn, std::iter::once(source.clone()), &arena)

.out_e("RelatesTo").collect::<Result<Vec<_>, _>>()?;
    let target = G::from_iter(&db, &txn, std::iter::once(source.clone()), &arena)

.out_node("RelatesTo").collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "source": Get_relationsSourceReturnType {
        id: uuid_str(source.id(), &arena),
        label: source.label(),
        memory_id: source.get_property("memory_id"),
        kind: source.get_property("kind"),
        title: source.get_property("title"),
        content: source.get_property("content"),
        summary: source.get_property("summary"),
        tags: source.get_property("tags"),
        source: source.get_property("source"),
        scope: source.get_property("scope"),
        importance: source.get_property("importance"),
        status: source.get_property("status"),
        privacy: source.get_property("privacy"),
        project_id: source.get_property("project_id"),
        session_id: source.get_property("session_id"),
        created_by: source.get_property("created_by"),
        created_at: source.get_property("created_at"),
        updated_at: source.get_property("updated_at"),
        accessed_at: source.get_property("accessed_at"),
        verification: source.get_property("verification"),
    },
    "target": target.iter().map(|target| Get_relationsTargetReturnType {
        id: uuid_str(target.id(), &arena),
        label: target.label(),
        memory_id: target.get_property("memory_id"),
        kind: target.get_property("kind"),
        title: target.get_property("title"),
        content: target.get_property("content"),
        summary: target.get_property("summary"),
        tags: target.get_property("tags"),
        source: target.get_property("source"),
        scope: target.get_property("scope"),
        importance: target.get_property("importance"),
        status: target.get_property("status"),
        privacy: target.get_property("privacy"),
        project_id: target.get_property("project_id"),
        session_id: target.get_property("session_id"),
        created_by: target.get_property("created_by"),
        created_at: target.get_property("created_at"),
        updated_at: target.get_property("updated_at"),
        accessed_at: target.get_property("accessed_at"),
        verification: target.get_property("verification"),
    }).collect::<Vec<_>>(),
    "edges": edges.iter().map(|edge| Get_relationsEdgesReturnType {
        id: uuid_str(edge.id(), &arena),
        label: edge.label(),
        from_node: uuid_str(edge.from_node(), &arena),
        to_node: uuid_str(edge.to_node(), &arena),
        relation_type: edge.get_property("relation_type"),
        strength: edge.get_property("strength"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct get_memoriesInput {

pub ids: Vec<String>
}
#[derive(Serialize, Default)]
pub struct Get_memoriesMemoryReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[handler]
pub fn get_memories (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_memoriesInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let memory = G::new(&db, &txn, &arena)
.n_from_type("Memory")

.filter_ref(|val, txn|{
                if let Ok(val) = val {
                    Ok(val
                    .get_property("memory_id")
                    .map_or(false, |v| v.is_in(&data.ids)))
                } else {
                    Ok(false)
                }
            }).collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "memory": memory.iter().map(|memory| Get_memoriesMemoryReturnType {
        id: uuid_str(memory.id(), &arena),
        label: memory.label(),
        memory_id: memory.get_property("memory_id"),
        kind: memory.get_property("kind"),
        title: memory.get_property("title"),
        content: memory.get_property("content"),
        summary: memory.get_property("summary"),
        tags: memory.get_property("tags"),
        source: memory.get_property("source"),
        scope: memory.get_property("scope"),
        importance: memory.get_property("importance"),
        status: memory.get_property("status"),
        privacy: memory.get_property("privacy"),
        project_id: memory.get_property("project_id"),
        session_id: memory.get_property("session_id"),
        created_by: memory.get_property("created_by"),
        created_at: memory.get_property("created_at"),
        updated_at: memory.get_property("updated_at"),
        accessed_at: memory.get_property("accessed_at"),
        verification: memory.get_property("verification"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct search_memoriesInput {

pub embedding: Vec<f64>,
pub limit: i64
}
#[derive(Serialize, Default)]
pub struct Search_memoriesResultsReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub data: &'a [f64],
    pub score: f64,
    pub memory_id: Option<&'a Value>,
    pub title: Option<&'a Value>,
}

#[handler]
pub fn search_memories (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<search_memoriesInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let results = G::new(&db, &txn, &arena)
.search_v::<fn(&HVector, &RoTxn) -> bool, _>(&data.embedding, data.limit.clone(), "MemoryEmbedding", None).collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "results": results.iter().map(|result| Search_memoriesResultsReturnType {
        id: uuid_str(result.id(), &arena),
        label: result.label(),
        data: result.data(),
        score: result.score(),
        memory_id: result.get_property("memory_id"),
        title: result.get_property("title"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct timelineInput {

pub limit: i64
}
#[derive(Serialize, Default)]
pub struct TimelineMemoryReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[handler]
pub fn timeline (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<timelineInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let memory = G::new(&db, &txn, &arena)
.n_from_type("Memory")

.range(0, data.limit.clone()).collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "memory": memory.iter().map(|memory| TimelineMemoryReturnType {
        id: uuid_str(memory.id(), &arena),
        label: memory.label(),
        memory_id: memory.get_property("memory_id"),
        kind: memory.get_property("kind"),
        title: memory.get_property("title"),
        content: memory.get_property("content"),
        summary: memory.get_property("summary"),
        tags: memory.get_property("tags"),
        source: memory.get_property("source"),
        scope: memory.get_property("scope"),
        importance: memory.get_property("importance"),
        status: memory.get_property("status"),
        privacy: memory.get_property("privacy"),
        project_id: memory.get_property("project_id"),
        session_id: memory.get_property("session_id"),
        created_by: memory.get_property("created_by"),
        created_at: memory.get_property("created_at"),
        updated_at: memory.get_property("updated_at"),
        accessed_at: memory.get_property("accessed_at"),
        verification: memory.get_property("verification"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct save_memoryInput {

pub id: String,
pub kind: String,
pub title: String,
pub content: String,
pub summary: String,
pub tags: String,
pub source: String,
pub scope: String,
pub importance: f64,
pub status: String,
pub privacy: String,
pub project_id: String,
pub session_id: String,
pub created_by: String,
pub created_at: String,
pub updated_at: String,
pub accessed_at: String,
pub verification: String,
pub embedding: Vec<f64>
}
#[derive(Serialize, Default)]
pub struct Save_memoryMemoryReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[handler(is_write)]
pub fn save_memory (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<save_memoryInput>(&input.request.body)?;
let arena = Bump::new();
let mut txn = db.graph_env.write_txn().map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let memory = G::new_mut(&db, &arena, &mut txn)
.add_n("Memory", Some(ImmutablePropertiesMap::new(18, vec![("verification", Value::from(&data.verification)), ("memory_id", Value::from(&data.id)), ("kind", Value::from(&data.kind)), ("created_by", Value::from(&data.created_by)), ("title", Value::from(&data.title)), ("content", Value::from(&data.content)), ("scope", Value::from(&data.scope)), ("updated_at", Value::from(&data.updated_at)), ("importance", Value::from(&data.importance)), ("privacy", Value::from(&data.privacy)), ("summary", Value::from(&data.summary)), ("created_at", Value::from(&data.created_at)), ("accessed_at", Value::from(&data.accessed_at)), ("project_id", Value::from(&data.project_id)), ("tags", Value::from(&data.tags)), ("session_id", Value::from(&data.session_id)), ("source", Value::from(&data.source)), ("status", Value::from(&data.status))].into_iter(), &arena)), Some(&["memory_id", "project_id", "session_id"])).collect_to_obj()?;
    let memory_vec = G::new_mut(&db, &arena, &mut txn)
.insert_v::<fn(&HVector, &RoTxn) -> bool>(&data.embedding, "MemoryEmbedding", Some(ImmutablePropertiesMap::new(2, vec![("title", Value::from(data.title.clone())), ("memory_id", Value::from(data.id.clone()))].into_iter(), &arena))).collect_to_obj()?;
let response = json!({
    "memory": Save_memoryMemoryReturnType {
        id: uuid_str(memory.id(), &arena),
        label: memory.label(),
        memory_id: memory.get_property("memory_id"),
        kind: memory.get_property("kind"),
        title: memory.get_property("title"),
        content: memory.get_property("content"),
        summary: memory.get_property("summary"),
        tags: memory.get_property("tags"),
        source: memory.get_property("source"),
        scope: memory.get_property("scope"),
        importance: memory.get_property("importance"),
        status: memory.get_property("status"),
        privacy: memory.get_property("privacy"),
        project_id: memory.get_property("project_id"),
        session_id: memory.get_property("session_id"),
        created_by: memory.get_property("created_by"),
        created_at: memory.get_property("created_at"),
        updated_at: memory.get_property("updated_at"),
        accessed_at: memory.get_property("accessed_at"),
        verification: memory.get_property("verification"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct save_memory_nodeInput {

pub id: String,
pub kind: String,
pub title: String,
pub content: String,
pub summary: String,
pub tags: String,
pub source: String,
pub scope: String,
pub importance: f64,
pub status: String,
pub privacy: String,
pub project_id: String,
pub session_id: String,
pub created_by: String,
pub created_at: String,
pub updated_at: String,
pub accessed_at: String,
pub verification: String
}
#[derive(Serialize, Default)]
pub struct Save_memory_nodeMemoryReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub memory_id: Option<&'a Value>,
    pub kind: Option<&'a Value>,
    pub title: Option<&'a Value>,
    pub content: Option<&'a Value>,
    pub summary: Option<&'a Value>,
    pub tags: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub scope: Option<&'a Value>,
    pub importance: Option<&'a Value>,
    pub status: Option<&'a Value>,
    pub privacy: Option<&'a Value>,
    pub project_id: Option<&'a Value>,
    pub session_id: Option<&'a Value>,
    pub created_by: Option<&'a Value>,
    pub created_at: Option<&'a Value>,
    pub updated_at: Option<&'a Value>,
    pub accessed_at: Option<&'a Value>,
    pub verification: Option<&'a Value>,
}

#[handler(is_write)]
pub fn save_memory_node (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<save_memory_nodeInput>(&input.request.body)?;
let arena = Bump::new();
let mut txn = db.graph_env.write_txn().map_err(|e| GraphError::New(format!("Failed to start write transaction: {:?}", e)))?;
    let memory = G::new_mut(&db, &arena, &mut txn)
.add_n("Memory", Some(ImmutablePropertiesMap::new(18, vec![("scope", Value::from(&data.scope)), ("updated_at", Value::from(&data.updated_at)), ("source", Value::from(&data.source)), ("kind", Value::from(&data.kind)), ("privacy", Value::from(&data.privacy)), ("accessed_at", Value::from(&data.accessed_at)), ("content", Value::from(&data.content)), ("memory_id", Value::from(&data.id)), ("title", Value::from(&data.title)), ("created_by", Value::from(&data.created_by)), ("summary", Value::from(&data.summary)), ("session_id", Value::from(&data.session_id)), ("created_at", Value::from(&data.created_at)), ("importance", Value::from(&data.importance)), ("status", Value::from(&data.status)), ("tags", Value::from(&data.tags)), ("verification", Value::from(&data.verification)), ("project_id", Value::from(&data.project_id))].into_iter(), &arena)), Some(&["memory_id", "project_id", "session_id"])).collect_to_obj()?;
let response = json!({
    "memory": Save_memory_nodeMemoryReturnType {
        id: uuid_str(memory.id(), &arena),
        label: memory.label(),
        memory_id: memory.get_property("memory_id"),
        kind: memory.get_property("kind"),
        title: memory.get_property("title"),
        content: memory.get_property("content"),
        summary: memory.get_property("summary"),
        tags: memory.get_property("tags"),
        source: memory.get_property("source"),
        scope: memory.get_property("scope"),
        importance: memory.get_property("importance"),
        status: memory.get_property("status"),
        privacy: memory.get_property("privacy"),
        project_id: memory.get_property("project_id"),
        session_id: memory.get_property("session_id"),
        created_by: memory.get_property("created_by"),
        created_at: memory.get_property("created_at"),
        updated_at: memory.get_property("updated_at"),
        accessed_at: memory.get_property("accessed_at"),
        verification: memory.get_property("verification"),
    }
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}


