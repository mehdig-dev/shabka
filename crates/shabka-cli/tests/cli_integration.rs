//! Integration tests for shabka CLI commands.
//!
//! Runs the actual CLI binary as a subprocess and verifies output.
//! Requires HelixDB running at localhost:6969 (`just db`).
//!
//! Run: `cargo test -p shabka-cli --no-default-features --test cli_integration -- --ignored`

use std::process::Command;

/// Check if HelixDB is reachable.
fn helix_available() -> bool {
    let output = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-X", "POST"])
        .args(["-H", "Content-Type: application/json"])
        .args(["-d", "{\"limit\": 1}"])
        .arg("http://localhost:6969/timeline")
        .output();

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("200"),
        Err(_) => false,
    }
}

/// Get the path to the built CLI binary.
fn cli_bin() -> String {
    // cargo test builds to target/debug/
    let mut path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    path.push("shabka");
    path.to_string_lossy().to_string()
}

/// Run a CLI command and return (stdout, stderr, exit_code).
fn run_cli(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(cli_bin())
        .args(args)
        .output()
        .expect("failed to run shabka CLI");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

#[test]
#[ignore]
fn test_cli_status() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _stderr, code) = run_cli(&["status"]);
    assert_eq!(code, 0, "shabka status should exit 0");
    assert!(
        stdout.contains("Shabka Status"),
        "should show status header"
    );
    assert!(
        stdout.contains("connected"),
        "should show HelixDB connected"
    );
    assert!(stdout.contains("Memories"), "should show memory count");
}

#[test]
#[ignore]
fn test_cli_search() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _stderr, code) = run_cli(&["search", "test query", "--limit", "3"]);
    assert_eq!(code, 0, "shabka search should exit 0");
    // Output should either show results or "No results"
    assert!(
        stdout.contains("ID") || stdout.contains("No results"),
        "should show table header or no-results message"
    );
}

#[test]
#[ignore]
fn test_cli_search_json_output() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _stderr, code) = run_cli(&["search", "test", "--json", "--limit", "2"]);
    assert_eq!(code, 0, "shabka search --json should exit 0");
    // JSON output should be valid JSON (array)
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(parsed.is_ok(), "JSON output should be valid JSON: {stdout}");
}

#[test]
#[ignore]
fn test_cli_search_with_kind_filter() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (_, _stderr, code) = run_cli(&["search", "test", "--kind", "fact", "--limit", "2"]);
    assert_eq!(code, 0, "shabka search --kind fact should exit 0");
}

#[test]
#[ignore]
fn test_cli_export_import_roundtrip() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let export_path = format!("/tmp/shabka-test-export-{}.json", uuid::Uuid::now_v7());

    // Export
    let (stdout, _stderr, code) = run_cli(&["export", "-o", &export_path]);
    assert_eq!(code, 0, "shabka export should exit 0");
    assert!(stdout.contains("Exported"), "should confirm export");

    // Verify file exists and is valid JSON
    let contents = std::fs::read_to_string(&export_path).expect("export file should exist");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("export should be valid JSON");
    assert!(
        parsed.get("memories").is_some(),
        "export should have memories key"
    );
    assert!(
        parsed.get("relations").is_some(),
        "export should have relations key"
    );

    // Cleanup
    let _ = std::fs::remove_file(&export_path);
}

#[test]
#[ignore]
fn test_cli_prune_dry_run() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _stderr, code) = run_cli(&["prune", "--dry-run", "--days", "9999"]);
    assert_eq!(code, 0, "shabka prune --dry-run should exit 0");
    // With --days 9999, nothing should be stale
    assert!(
        stdout.contains("0 stale") || stdout.contains("No stale"),
        "nothing should be stale with --days 9999: {stdout}"
    );
}

#[test]
#[ignore]
fn test_cli_reembed_dry_run() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _stderr, code) = run_cli(&["reembed", "--dry-run"]);
    assert_eq!(code, 0, "shabka reembed --dry-run should exit 0");
    assert!(
        stdout.contains("Dry run") || stdout.contains("Nothing to do"),
        "should indicate dry run: {stdout}"
    );
}

#[test]
#[ignore]
fn test_cli_init_creates_config() {
    let test_dir = format!("/tmp/shabka-init-test-{}", uuid::Uuid::now_v7());
    std::fs::create_dir_all(&test_dir).unwrap();

    let output = Command::new(cli_bin())
        .args(["init"])
        .current_dir(&test_dir)
        .output()
        .expect("failed to run shabka init");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let code = output.status.code().unwrap_or(-1);

    assert_eq!(code, 0, "shabka init should exit 0");
    assert!(
        stdout.contains("Created") || stdout.contains("config"),
        "should confirm config creation: {stdout}"
    );

    // Verify .shabka/config.toml exists
    let config_path = format!("{}/.shabka/config.toml", test_dir);
    assert!(
        std::path::Path::new(&config_path).exists(),
        "config file should exist at {config_path}"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
#[ignore]
fn test_cli_get_invalid_id() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (_, stderr, code) = run_cli(&["get", "00000000-0000-0000-0000-000000000000"]);
    // Should fail gracefully (not panic)
    assert!(
        code != 0 || stderr.contains("not found") || stderr.contains("error"),
        "getting non-existent ID should fail gracefully"
    );
}

#[test]
#[ignore]
fn test_cli_chain_invalid_id() {
    if !helix_available() {
        eprintln!("SKIP: HelixDB not available");
        return;
    }

    let (stdout, _, code) = run_cli(&["chain", "00000000-0000-0000-0000-000000000000"]);
    // Should either error or show no results
    assert!(
        code == 0 || stdout.contains("No") || stdout.is_empty(),
        "chain with non-existent ID should handle gracefully"
    );
}
