use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Relationship between two memories in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relation_type: RelationType,
    pub strength: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    CausedBy,
    Fixes,
    Supersedes,
    Related,
    Contradicts,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CausedBy => write!(f, "caused_by"),
            Self::Fixes => write!(f, "fixes"),
            Self::Supersedes => write!(f, "supersedes"),
            Self::Related => write!(f, "related"),
            Self::Contradicts => write!(f, "contradicts"),
        }
    }
}

impl std::str::FromStr for RelationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "caused_by" => Ok(Self::CausedBy),
            "fixes" => Ok(Self::Fixes),
            "supersedes" => Ok(Self::Supersedes),
            "related" => Ok(Self::Related),
            "contradicts" => Ok(Self::Contradicts),
            _ => Err(format!("unknown relation type: {s}")),
        }
    }
}

/// A memory with its graph neighbors, for Layer 3 (full detail) retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWithRelations {
    pub memory: super::Memory,
    pub relations: Vec<MemoryRelation>,
}
