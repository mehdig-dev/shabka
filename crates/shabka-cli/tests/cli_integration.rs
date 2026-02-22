//! CLI integration tests — run the actual shabka binary.
//! Marked `#[ignore]` to skip in normal `cargo test`.

use std::process::Command;

fn shabka() -> Command {
    Command::new(env!("CARGO_BIN_EXE_shabka"))
}

#[test]
#[ignore]
fn test_cli_status_output() {
    let output = shabka().arg("status").output().expect("failed to execute");
    assert!(
        output.status.success(),
        "shabka status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore]
fn test_cli_list_json() {
    let output = shabka()
        .args(["list", "--json"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be valid JSON array
    let _: Vec<serde_json::Value> =
        serde_json::from_str(stdout.trim()).expect("invalid JSON output");
}

#[test]
#[ignore]
fn test_cli_delete_requires_confirm() {
    let output = shabka()
        .args(["delete", "--kind", "error"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "delete without --confirm should fail"
    );
}

#[test]
#[ignore]
fn test_cli_search_fuzzy() {
    let output = shabka()
        .args(["search", "authentcation", "--json"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
}

#[test]
#[ignore]
fn test_cli_demo_lifecycle() {
    let demo = shabka().arg("demo").output().expect("failed to execute");
    assert!(
        demo.status.success(),
        "demo failed: {}",
        String::from_utf8_lossy(&demo.stderr)
    );

    let clean = shabka()
        .args(["demo", "--clean"])
        .output()
        .expect("failed to execute");
    assert!(clean.status.success(), "demo --clean failed");
}

#[test]
#[ignore]
fn test_cli_init_creates_config() {
    let tmp = std::env::temp_dir().join(format!("shabka-init-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&tmp).unwrap();

    let output = shabka()
        .args(["init", "--provider", "hash"])
        .current_dir(&tmp)
        .output()
        .expect("failed to execute");
    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
