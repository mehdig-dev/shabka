use crate::model::memory::{
    validate_create_input, validate_update_input, MAX_CONTENT_LENGTH, MAX_TITLE_LENGTH,
};
use crate::model::*;

#[test]
fn test_memory_creation() {
    let memory = Memory::new(
        "Test memory".to_string(),
        "This is test content".to_string(),
        MemoryKind::Observation,
        "test-user".to_string(),
    );

    assert_eq!(memory.title, "Test memory");
    assert_eq!(memory.content, "This is test content");
    assert_eq!(memory.kind, MemoryKind::Observation);
    assert_eq!(memory.importance, 0.5);
    assert_eq!(memory.status, MemoryStatus::Active);
    assert_eq!(memory.privacy, MemoryPrivacy::Private);
    assert!(memory.tags.is_empty());
    assert!(memory.project_id.is_none());
}

#[test]
fn test_memory_builder() {
    let memory = Memory::new(
        "Pattern".to_string(),
        "Always use builder pattern".to_string(),
        MemoryKind::Pattern,
        "user".to_string(),
    )
    .with_tags(vec!["rust".to_string(), "patterns".to_string()])
    .with_importance(0.9)
    .with_project("shabka".to_string());

    assert_eq!(memory.tags.len(), 2);
    assert_eq!(memory.importance, 0.9);
    assert_eq!(memory.project_id.as_deref(), Some("shabka"));
}

#[test]
fn test_importance_clamping() {
    let m1 = Memory::new("t".into(), "c".into(), MemoryKind::Fact, "u".into()).with_importance(1.5);
    assert_eq!(m1.importance, 1.0);

    let m2 =
        Memory::new("t".into(), "c".into(), MemoryKind::Fact, "u".into()).with_importance(-0.5);
    assert_eq!(m2.importance, 0.0);
}

#[test]
fn test_memory_summary_truncation() {
    let long_content = "x".repeat(500);
    let memory = Memory::new(
        "Long".to_string(),
        long_content,
        MemoryKind::Observation,
        "user".to_string(),
    );
    assert!(memory.summary.len() <= 203); // 200 + "..."
    assert!(memory.summary.ends_with("..."));
}

#[test]
fn test_memory_kind_roundtrip() {
    let kinds = [
        MemoryKind::Observation,
        MemoryKind::Decision,
        MemoryKind::Pattern,
        MemoryKind::Error,
        MemoryKind::Fix,
        MemoryKind::Preference,
        MemoryKind::Fact,
        MemoryKind::Lesson,
        MemoryKind::Todo,
        MemoryKind::Procedure,
    ];

    for kind in kinds {
        let s = kind.to_string();
        let parsed: MemoryKind = s.parse().unwrap();
        assert_eq!(kind, parsed);
    }
}

#[test]
fn test_relation_type_roundtrip() {
    let types = [
        RelationType::CausedBy,
        RelationType::Fixes,
        RelationType::Supersedes,
        RelationType::Related,
        RelationType::Contradicts,
    ];

    for rt in types {
        let s = rt.to_string();
        let parsed: RelationType = s.parse().unwrap();
        assert_eq!(rt, parsed);
    }
}

#[test]
fn test_memory_index_from() {
    let memory = Memory::new(
        "Test".to_string(),
        "Content".to_string(),
        MemoryKind::Fact,
        "user".to_string(),
    )
    .with_tags(vec!["tag1".to_string()]);

    let index = MemoryIndex::from((&memory, 0.95));
    assert_eq!(index.id, memory.id);
    assert_eq!(index.title, "Test");
    assert_eq!(index.kind, MemoryKind::Fact);
    assert_eq!(index.score, 0.95);
    assert_eq!(index.tags, vec!["tag1"]);
}

#[test]
fn test_embedding_text() {
    let memory = Memory::new(
        "Title".to_string(),
        "Full content here".to_string(),
        MemoryKind::Observation,
        "user".to_string(),
    )
    .with_tags(vec!["rust".to_string(), "code".to_string()]);

    let text = memory.embedding_text();
    assert!(text.contains("Title"));
    assert!(text.contains("rust, code"));
}

#[test]
fn test_memory_serde_roundtrip() {
    let memory = Memory::new(
        "Serde test".to_string(),
        "Testing serialization".to_string(),
        MemoryKind::Lesson,
        "user".to_string(),
    )
    .with_tags(vec!["test".to_string()])
    .with_importance(0.8);

    let json = serde_json::to_string(&memory).unwrap();
    let deserialized: Memory = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, memory.id);
    assert_eq!(deserialized.title, memory.title);
    assert_eq!(deserialized.kind, memory.kind);
    assert_eq!(deserialized.importance, memory.importance);
}

#[test]
fn test_session_creation() {
    let session = Session::new(Some("test-project".to_string()));
    assert_eq!(session.project_id.as_deref(), Some("test-project"));
    assert!(session.ended_at.is_none());
    assert!(session.summary.is_none());
    assert_eq!(session.memory_count, 0);
}

#[test]
fn test_create_memory_input_defaults() {
    let json = r#"{"title":"Test","content":"Content","kind":"observation"}"#;
    let input: CreateMemoryInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.importance, 0.5);
    assert!(input.tags.is_empty());
    assert!(input.scope.is_none());
    assert!(input.privacy.is_none());
}

#[test]
fn test_memory_privacy_roundtrip() {
    use std::str::FromStr;

    for (s, expected) in [
        ("public", MemoryPrivacy::Public),
        ("team", MemoryPrivacy::Team),
        ("private", MemoryPrivacy::Private),
    ] {
        let parsed = MemoryPrivacy::from_str(s).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), s);
    }

    assert!(MemoryPrivacy::from_str("invalid").is_err());
}

// -- Validation tests --

#[test]
fn test_validate_create_empty_title_rejected() {
    let result = validate_create_input("", "content", 0.5);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("title cannot be empty"));
}

#[test]
fn test_validate_create_whitespace_title_rejected() {
    let result = validate_create_input("   \t\n  ", "content", 0.5);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("title cannot be empty"));
}

#[test]
fn test_validate_create_title_too_long() {
    let long_title = "a".repeat(MAX_TITLE_LENGTH + 1);
    let result = validate_create_input(&long_title, "content", 0.5);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("maximum length"));
}

#[test]
fn test_validate_create_content_too_long() {
    let long_content = "a".repeat(MAX_CONTENT_LENGTH + 1);
    let result = validate_create_input("title", &long_content, 0.5);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("maximum length"));
}

#[test]
fn test_validate_create_importance_below_zero() {
    let result = validate_create_input("title", "content", -0.1);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("importance"));
}

#[test]
fn test_validate_create_importance_above_one() {
    let result = validate_create_input("title", "content", 1.1);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("importance"));
}

#[test]
fn test_validate_create_valid_input() {
    let result = validate_create_input("Valid title", "Valid content", 0.5);
    assert!(result.is_ok());
}

#[test]
fn test_validate_update_none_fields_pass() {
    let input = UpdateMemoryInput::default();
    let result = validate_update_input(&input);
    assert!(result.is_ok());
}

#[test]
fn test_validate_update_empty_title_rejected() {
    let input = UpdateMemoryInput {
        title: Some("".into()),
        ..Default::default()
    };
    let result = validate_update_input(&input);
    assert!(result.is_err());
}

#[test]
fn test_verification_status_roundtrip() {
    use std::str::FromStr;

    for (s, expected) in [
        ("unverified", VerificationStatus::Unverified),
        ("verified", VerificationStatus::Verified),
        ("disputed", VerificationStatus::Disputed),
        ("outdated", VerificationStatus::Outdated),
    ] {
        let parsed = VerificationStatus::from_str(s).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), s);
    }

    assert!(VerificationStatus::from_str("invalid").is_err());
}

#[test]
fn test_memory_default_verification() {
    let memory = Memory::new(
        "Test".to_string(),
        "Content".to_string(),
        MemoryKind::Fact,
        "user".to_string(),
    );
    assert_eq!(memory.verification, VerificationStatus::Unverified);
}

#[test]
fn test_memory_serde_without_verification() {
    let memory = Memory::new(
        "Compat test".to_string(),
        "Backward compatibility".to_string(),
        MemoryKind::Lesson,
        "user".to_string(),
    );

    let mut json: serde_json::Value = serde_json::to_value(&memory).unwrap();
    json.as_object_mut().unwrap().remove("verification");

    let deserialized: Memory = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.verification, VerificationStatus::Unverified);
}

#[test]
fn test_procedure_kind_parse() {
    use std::str::FromStr;
    let kind = MemoryKind::from_str("procedure").unwrap();
    assert_eq!(kind, MemoryKind::Procedure);
    assert_eq!(kind.to_string(), "procedure");
}

#[test]
fn test_pending_status_display() {
    let status = MemoryStatus::Pending;
    assert_eq!(status.to_string(), "pending");
}

#[test]
fn test_procedure_kind_serde() {
    let kind = MemoryKind::Procedure;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, "\"procedure\"");
    let parsed: MemoryKind = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, MemoryKind::Procedure);
}

#[test]
fn test_pending_status_serde() {
    let status = MemoryStatus::Pending;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"pending\"");
    let parsed: MemoryStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, MemoryStatus::Pending);
}
