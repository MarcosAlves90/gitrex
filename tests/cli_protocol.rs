use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::{json, Value};
use tempfile::TempDir;

mod support;

use support::{commit_all, configure_user, init_repo, write_file, TestRepo};

fn committed_repo() -> (TempDir, TestRepo) {
    let directory = TempDir::new().unwrap();
    let repo = init_repo(directory.path(), "main");
    configure_user(&repo);
    write_file(
        directory.path(),
        "README.md",
        "GitRex machine protocol fixture\n",
    );
    commit_all(&repo, "initial commit");
    (directory, repo)
}

fn run_cli(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gitrex"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/protocol/v1")
        .join(name);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn parse_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not valid JSON ({error}); stdout={:?}; stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_success_envelope(output: &Output, operation: &str) -> Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_json(output);
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["operation"], operation);
    assert_eq!(response["ok"], true);
    response
}

fn normalize_oid(field: &mut Value) {
    let oid = field.as_str().expect("commit id should be a string");
    assert_eq!(oid.len(), 40, "expected SHA-1 object ID, got {oid:?}");
    assert!(oid.bytes().all(|byte| byte.is_ascii_hexdigit()));
    *field = json!("<oid>");
}

#[test]
fn machine_protocol_status_matches_v1_fixture() {
    let (directory, _) = committed_repo();
    write_file(directory.path(), "notes.md", "untracked note\n");

    let output = run_cli(directory.path(), &["status", "--format", "json"]);
    let response = assert_success_envelope(&output, "status");
    assert_eq!(response, fixture("status.json"));
}

#[test]
fn machine_protocol_branch_matches_v1_fixture() {
    let (directory, _) = committed_repo();

    let output = run_cli(directory.path(), &["branch", "--format", "json"]);
    let mut response = assert_success_envelope(&output, "branch");
    normalize_oid(&mut response["data"]["branches"][0]["commit"]);
    assert_eq!(response, fixture("branch.json"));
}

#[test]
fn machine_protocol_log_matches_v1_fixture() {
    let (directory, _) = committed_repo();

    let output = run_cli(
        directory.path(),
        &["log", "--limit", "1", "--format", "json"],
    );
    let mut response = assert_success_envelope(&output, "log");
    normalize_oid(&mut response["data"]["commits"][0]["hash"]);
    let date = response["data"]["commits"][0]["date"]
        .as_str()
        .expect("commit date should be a string");
    assert_eq!(date.len(), 10);
    assert_eq!(&date[4..5], "-");
    assert_eq!(&date[7..8], "-");
    response["data"]["commits"][0]["date"] = json!("<date>");
    assert_eq!(response, fixture("log.json"));
}

#[test]
fn machine_protocol_not_repository_failure_matches_v1_fixture() {
    let directory = TempDir::new().unwrap();
    let output = run_cli(directory.path(), &["status", "--format", "json"]);

    assert_eq!(output.status.code(), Some(1));
    let response = parse_json(&output);
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["operation"], "status");
    assert_eq!(response["ok"], false);
    assert_eq!(response, fixture("not-repository.json"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("repository not found"));
}

#[test]
fn machine_protocol_capabilities_matches_v1_fixture_without_git() {
    let directory = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gitrex"))
        .args(["capabilities", "--format", "json"])
        .current_dir(directory.path())
        .env("PATH", directory.path())
        .output()
        .unwrap();
    let mut response = assert_success_envelope(&output, "capabilities");
    assert_eq!(
        response["data"]["gitrex_version"],
        env!("CARGO_PKG_VERSION")
    );
    response["data"]["gitrex_version"] = json!("<version>");
    assert_eq!(response, fixture("capabilities.json"));

    let repeated = Command::new(env!("CARGO_BIN_EXE_gitrex"))
        .args(["capabilities", "--format", "json"])
        .current_dir(directory.path())
        .env("PATH", directory.path())
        .output()
        .unwrap();
    assert_eq!(output.stdout, repeated.stdout);
}

#[test]
fn machine_protocol_status_keeps_human_output_as_default() {
    let (directory, _) = committed_repo();
    write_file(directory.path(), "notes.md", "untracked note\n");

    let output = run_cli(directory.path(), &["status"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "branch: main\nworking tree:\n  ?? notes.md\n"
    );
}

#[test]
fn machine_protocol_invalid_format_uses_usage_exit_code() {
    let directory = TempDir::new().unwrap();
    let output = run_cli(directory.path(), &["status", "--format", "yaml"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("json"));
}
