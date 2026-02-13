use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub project_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub memory_count: usize,
}

impl Session {
    pub fn new(project_id: Option<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            started_at: Utc::now(),
            ended_at: None,
            summary: None,
            memory_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub embedding_model: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Project {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            path: None,
            embedding_model: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}
