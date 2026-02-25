use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Result, ShabkaError};

pub const MAX_TITLE_LENGTH: usize = 500;
pub const MAX_CONTENT_LENGTH: usize = 50_000;

/// Validate inputs for creating a new memory.
pub fn validate_create_input(title: &str, content: &str, importance: f32) -> Result<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(ShabkaError::InvalidInput("title cannot be empty".into()));
    }
    if trimmed.len() > MAX_TITLE_LENGTH {
        return Err(ShabkaError::InvalidInput(format!(
            "title exceeds maximum length of {MAX_TITLE_LENGTH} characters"
        )));
    }
    if content.len() > MAX_CONTENT_LENGTH {
        return Err(ShabkaError::InvalidInput(format!(
            "content exceeds maximum length of {MAX_CONTENT_LENGTH} characters"
        )));
    }
    if !(0.0..=1.0).contains(&importance) {
        return Err(ShabkaError::InvalidInput(
            "importance must be between 0.0 and 1.0".into(),
        ));
    }
    Ok(())
}

/// Validate inputs for updating an existing memory.
pub fn validate_update_input(input: &UpdateMemoryInput) -> Result<()> {
    if let Some(ref title) = input.title {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(ShabkaError::InvalidInput("title cannot be empty".into()));
        }
        if trimmed.len() > MAX_TITLE_LENGTH {
            return Err(ShabkaError::InvalidInput(format!(
                "title exceeds maximum length of {MAX_TITLE_LENGTH} characters"
            )));
        }
    }
    if let Some(ref content) = input.content {
        if content.len() > MAX_CONTENT_LENGTH {
            return Err(ShabkaError::InvalidInput(format!(
                "content exceeds maximum length of {MAX_CONTENT_LENGTH} characters"
            )));
        }
    }
    if let Some(importance) = input.importance {
        if !(0.0..=1.0).contains(&importance) {
            return Err(ShabkaError::InvalidInput(
                "importance must be between 0.0 and 1.0".into(),
            ));
        }
    }
    Ok(())
}

/// The core entity in Shabka. Represents a unit of captured knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub scope: MemoryScope,
    pub importance: f32,
    pub status: MemoryStatus,
    pub privacy: MemoryPrivacy,
    #[serde(default)]
    pub verification: VerificationStatus,
    pub project_id: Option<String>,
    pub session_id: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub accessed_at: DateTime<Utc>,
}

impl Memory {
    pub fn new(title: String, content: String, kind: MemoryKind, created_by: String) -> Self {
        let now = Utc::now();
        let summary = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.clone()
        };

        Self {
            id: Uuid::now_v7(),
            kind,
            title,
            content,
            summary,
            tags: Vec::new(),
            source: MemorySource::Manual,
            scope: MemoryScope::Global,
            importance: 0.5,
            status: MemoryStatus::Active,
            privacy: MemoryPrivacy::Private,
            verification: VerificationStatus::default(),
            project_id: None,
            session_id: None,
            created_by,
            created_at: now,
            updated_at: now,
            accessed_at: now,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_importance(mut self, importance: f32) -> Self {
        self.importance = importance.clamp(0.0, 1.0);
        self
    }

    pub fn with_scope(mut self, scope: MemoryScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_source(mut self, source: MemorySource) -> Self {
        self.source = source;
        self
    }

    pub fn with_project(mut self, project_id: String) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub fn with_session(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn with_summary(mut self, summary: String) -> Self {
        self.summary = summary;
        self
    }

    pub fn with_privacy(mut self, privacy: MemoryPrivacy) -> Self {
        self.privacy = privacy;
        self
    }

    pub fn with_verification(mut self, verification: VerificationStatus) -> Self {
        self.verification = verification;
        self
    }

    /// Text used for generating embeddings: title + summary + tags.
    pub fn embedding_text(&self) -> String {
        let tags = self.tags.join(", ");
        format!("{}\n{}\n{}", self.title, self.summary, tags)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Observation,
    Decision,
    Pattern,
    Error,
    Fix,
    Preference,
    Fact,
    Lesson,
    Todo,
    Procedure,
}

impl std::fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observation => write!(f, "observation"),
            Self::Decision => write!(f, "decision"),
            Self::Pattern => write!(f, "pattern"),
            Self::Error => write!(f, "error"),
            Self::Fix => write!(f, "fix"),
            Self::Preference => write!(f, "preference"),
            Self::Fact => write!(f, "fact"),
            Self::Lesson => write!(f, "lesson"),
            Self::Todo => write!(f, "todo"),
            Self::Procedure => write!(f, "procedure"),
        }
    }
}

impl std::str::FromStr for MemoryKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "observation" => Ok(Self::Observation),
            "decision" => Ok(Self::Decision),
            "pattern" => Ok(Self::Pattern),
            "error" => Ok(Self::Error),
            "fix" => Ok(Self::Fix),
            "preference" => Ok(Self::Preference),
            "fact" => Ok(Self::Fact),
            "lesson" => Ok(Self::Lesson),
            "todo" => Ok(Self::Todo),
            "procedure" => Ok(Self::Procedure),
            _ => Err(format!("unknown memory kind: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum MemorySource {
    Manual,
    AutoCapture { hook: String },
    Import,
    Derived { from: Uuid },
}

impl std::fmt::Display for MemorySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::AutoCapture { hook } => write!(f, "auto-capture ({hook})"),
            Self::Import => write!(f, "import"),
            Self::Derived { from } => write!(f, "derived ({from})"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum MemoryScope {
    Global,
    Project { id: String },
    Session { id: Uuid },
}

impl std::fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::Project { id } => write!(f, "project ({id})"),
            Self::Session { id } => write!(f, "session ({id})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    #[default]
    Active,
    Archived,
    Superseded,
    Pending,
}

impl std::fmt::Display for MemoryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Archived => write!(f, "archived"),
            Self::Superseded => write!(f, "superseded"),
            Self::Pending => write!(f, "pending"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPrivacy {
    Public,
    Team,
    Private,
}

impl std::fmt::Display for MemoryPrivacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Team => write!(f, "team"),
            Self::Private => write!(f, "private"),
        }
    }
}

impl std::str::FromStr for MemoryPrivacy {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "public" => Ok(Self::Public),
            "team" => Ok(Self::Team),
            "private" => Ok(Self::Private),
            _ => Err(format!("unknown privacy level: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    #[default]
    Unverified,
    Verified,
    Disputed,
    Outdated,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unverified => write!(f, "unverified"),
            Self::Verified => write!(f, "verified"),
            Self::Disputed => write!(f, "disputed"),
            Self::Outdated => write!(f, "outdated"),
        }
    }
}

impl std::str::FromStr for VerificationStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unverified" => Ok(Self::Unverified),
            "verified" => Ok(Self::Verified),
            "disputed" => Ok(Self::Disputed),
            "outdated" => Ok(Self::Outdated),
            _ => Err(format!("unknown verification status: {s}")),
        }
    }
}

/// Compact representation for Layer 1 search results (~50-100 tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryIndex {
    pub id: Uuid,
    pub title: String,
    pub kind: MemoryKind,
    pub created_at: DateTime<Utc>,
    pub score: f32,
    pub tags: Vec<String>,
    #[serde(default)]
    pub verification: VerificationStatus,
}

impl From<(&Memory, f32)> for MemoryIndex {
    fn from((memory, score): (&Memory, f32)) -> Self {
        Self {
            id: memory.id,
            title: memory.title.clone(),
            kind: memory.kind,
            created_at: memory.created_at,
            score,
            tags: memory.tags.clone(),
            verification: memory.verification,
        }
    }
}

/// Input for creating a new memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMemoryInput {
    pub title: String,
    pub content: String,
    pub kind: MemoryKind,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_importance")]
    pub importance: f32,
    #[serde(default)]
    pub scope: Option<MemoryScope>,
    #[serde(default)]
    pub related_to: Vec<Uuid>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub privacy: Option<MemoryPrivacy>,
}

fn default_importance() -> f32 {
    0.5
}

/// Input for updating an existing memory.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateMemoryInput {
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f32>,
    pub status: Option<MemoryStatus>,
    pub kind: Option<MemoryKind>,
    pub privacy: Option<MemoryPrivacy>,
    pub verification: Option<VerificationStatus>,
}

/// Search query parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

/// Timeline query parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineQuery {
    #[serde(default)]
    pub memory_id: Option<Uuid>,
    #[serde(default)]
    pub start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub session_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    #[serde(default)]
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub privacy: Option<MemoryPrivacy>,
    #[serde(default)]
    pub created_by: Option<String>,
}

impl Default for TimelineQuery {
    fn default() -> Self {
        Self {
            memory_id: None,
            start: None,
            end: None,
            session_id: None,
            limit: default_limit(),
            offset: 0,
            project_id: None,
            kind: None,
            status: None,
            privacy: None,
            created_by: None,
        }
    }
}

/// Timeline entry with context (~200-300 tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: Uuid,
    pub title: String,
    pub kind: MemoryKind,
    pub summary: String,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub session_id: Option<Uuid>,
    pub related_count: usize,
    pub privacy: MemoryPrivacy,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub status: MemoryStatus,
    #[serde(default)]
    pub verification: VerificationStatus,
}

impl From<(&Memory, usize)> for TimelineEntry {
    fn from((memory, related_count): (&Memory, usize)) -> Self {
        Self {
            id: memory.id,
            title: memory.title.clone(),
            kind: memory.kind,
            summary: memory.summary.clone(),
            importance: memory.importance,
            created_at: memory.created_at,
            session_id: memory.session_id,
            related_count,
            privacy: memory.privacy,
            created_by: memory.created_by.clone(),
            project_id: memory.project_id.clone(),
            status: memory.status,
            verification: memory.verification,
        }
    }
}
