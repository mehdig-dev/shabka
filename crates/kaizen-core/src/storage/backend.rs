use crate::error::Result;
use crate::model::*;
use uuid::Uuid;

/// Abstract storage backend. HelixDB is the primary implementation,
/// but this trait allows swapping to SQLite or other backends.
pub trait StorageBackend: Send + Sync {
    // -- Memory CRUD --

    fn save_memory(
        &self,
        memory: &Memory,
        embedding: Option<&[f32]>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn get_memory(&self, id: Uuid) -> impl std::future::Future<Output = Result<Memory>> + Send;

    fn get_memories(
        &self,
        ids: &[Uuid],
    ) -> impl std::future::Future<Output = Result<Vec<Memory>>> + Send;

    fn update_memory(
        &self,
        id: Uuid,
        input: &UpdateMemoryInput,
    ) -> impl std::future::Future<Output = Result<Memory>> + Send;

    fn delete_memory(&self, id: Uuid) -> impl std::future::Future<Output = Result<()>> + Send;

    // -- Search --

    /// Vector similarity search. Returns (memory, score) pairs.
    fn vector_search(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> impl std::future::Future<Output = Result<Vec<(Memory, f32)>>> + Send;

    // -- Timeline --

    fn timeline(
        &self,
        query: &TimelineQuery,
    ) -> impl std::future::Future<Output = Result<Vec<TimelineEntry>>> + Send;

    // -- Graph --

    fn add_relation(
        &self,
        relation: &MemoryRelation,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn get_relations(
        &self,
        memory_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Vec<MemoryRelation>>> + Send;

    /// Count outgoing relations for a batch of memory IDs.
    /// Returns (id, count) pairs for each input ID.
    fn count_relations(
        &self,
        memory_ids: &[Uuid],
    ) -> impl std::future::Future<Output = Result<Vec<(Uuid, usize)>>> + Send;

    // -- Session --

    fn save_session(
        &self,
        session: &Session,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn get_session(&self, id: Uuid) -> impl std::future::Future<Output = Result<Session>> + Send;
}
