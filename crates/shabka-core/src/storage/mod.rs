mod backend;
mod helix;
mod sqlite;

pub use backend::StorageBackend;
pub use helix::HelixStorage;
pub use sqlite::SqliteStorage;

use crate::config::ShabkaConfig;
use crate::error::{Result, ShabkaError};
use crate::model::*;
use uuid::Uuid;

/// Enum wrapper for storage backends. Dispatches to the concrete implementation.
/// Using an enum instead of `Box<dyn StorageBackend>` because the trait uses RPITIT.
pub enum Storage {
    Sqlite(SqliteStorage),
    Helix(HelixStorage),
}

impl StorageBackend for Storage {
    async fn save_memory(&self, memory: &Memory, embedding: Option<&[f32]>) -> Result<()> {
        match self {
            Storage::Sqlite(s) => s.save_memory(memory, embedding).await,
            Storage::Helix(s) => s.save_memory(memory, embedding).await,
        }
    }

    async fn get_memory(&self, id: Uuid) -> Result<Memory> {
        match self {
            Storage::Sqlite(s) => s.get_memory(id).await,
            Storage::Helix(s) => s.get_memory(id).await,
        }
    }

    async fn get_memories(&self, ids: &[Uuid]) -> Result<Vec<Memory>> {
        match self {
            Storage::Sqlite(s) => s.get_memories(ids).await,
            Storage::Helix(s) => s.get_memories(ids).await,
        }
    }

    async fn update_memory(&self, id: Uuid, input: &UpdateMemoryInput) -> Result<Memory> {
        match self {
            Storage::Sqlite(s) => s.update_memory(id, input).await,
            Storage::Helix(s) => s.update_memory(id, input).await,
        }
    }

    async fn delete_memory(&self, id: Uuid) -> Result<()> {
        match self {
            Storage::Sqlite(s) => s.delete_memory(id).await,
            Storage::Helix(s) => s.delete_memory(id).await,
        }
    }

    async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<(Memory, f32)>> {
        match self {
            Storage::Sqlite(s) => s.vector_search(embedding, limit).await,
            Storage::Helix(s) => s.vector_search(embedding, limit).await,
        }
    }

    async fn timeline(&self, query: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
        match self {
            Storage::Sqlite(s) => s.timeline(query).await,
            Storage::Helix(s) => s.timeline(query).await,
        }
    }

    async fn add_relation(&self, relation: &MemoryRelation) -> Result<()> {
        match self {
            Storage::Sqlite(s) => s.add_relation(relation).await,
            Storage::Helix(s) => s.add_relation(relation).await,
        }
    }

    async fn get_relations(&self, memory_id: Uuid) -> Result<Vec<MemoryRelation>> {
        match self {
            Storage::Sqlite(s) => s.get_relations(memory_id).await,
            Storage::Helix(s) => s.get_relations(memory_id).await,
        }
    }

    async fn count_relations(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        match self {
            Storage::Sqlite(s) => s.count_relations(memory_ids).await,
            Storage::Helix(s) => s.count_relations(memory_ids).await,
        }
    }

    async fn count_contradictions(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        match self {
            Storage::Sqlite(s) => s.count_contradictions(memory_ids).await,
            Storage::Helix(s) => s.count_contradictions(memory_ids).await,
        }
    }

    async fn save_session(&self, session: &Session) -> Result<()> {
        match self {
            Storage::Sqlite(s) => s.save_session(session).await,
            Storage::Helix(s) => s.save_session(session).await,
        }
    }

    async fn get_session(&self, id: Uuid) -> Result<Session> {
        match self {
            Storage::Sqlite(s) => s.get_session(id).await,
            Storage::Helix(s) => s.get_session(id).await,
        }
    }
}

impl Storage {
    /// Return `(schema_version, last_writer_version)` for SQLite, `None` for Helix.
    pub fn schema_info(&self) -> Option<(i32, Option<String>)> {
        match self {
            Storage::Sqlite(s) => s.schema_info().ok(),
            Storage::Helix(_) => None,
        }
    }
}

/// Create a storage backend from the given configuration.
pub fn create_backend(config: &ShabkaConfig) -> Result<Storage> {
    match config.storage.backend.as_str() {
        "sqlite" => {
            let path = match &config.storage.path {
                Some(p) => std::path::PathBuf::from(p),
                None => default_sqlite_path()?,
            };
            let storage = SqliteStorage::open(&path)?;
            Ok(Storage::Sqlite(storage))
        }
        "helix" => {
            let storage = HelixStorage::new(
                Some(&config.helix.url),
                Some(config.helix.port),
                config.helix.api_key.as_deref(),
            );
            Ok(Storage::Helix(storage))
        }
        other => Err(ShabkaError::Config(format!(
            "unknown storage backend: {other}"
        ))),
    }
}

/// Default SQLite path: `~/.config/shabka/shabka.db`
fn default_sqlite_path() -> Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("shabka").join("shabka.db"))
        .ok_or_else(|| ShabkaError::Config("cannot determine config directory".to_string()))
}
