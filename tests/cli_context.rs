use std::path::Path;
use std::process::{Command as ProcessCommand, Output};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn git(repository: &Path, arguments: &[&str]) -> Output {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .expect("git is installed for integration tests");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git_stdout(repository: &Path, arguments: &[&str]) -> String {
    String::from_utf8(git(repository, arguments).stdout)
        .expect("Git fixture output is UTF-8")
        .trim()
        .to_string()
}

fn repository() -> TempDir {
    let directory = tempfile::tempdir().expect("temporary repository directory");
    git(
        directory.path(),
        &["init", "--quiet", "--initial-branch=main"],
    );
    git(directory.path(), &["config", "user.name", "GitRex Test"]);
    git(
        directory.path(),
        &["config", "user.email", "gitrex-test@example.invalid"],
    );
    std::fs::write(directory.path().join("README.md"), "initial\n").expect("write test fixture");
    git(directory.path(), &["add", "--", "README.md"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "initial commit"],
    );
    directory
}

fn gitrex(repository: &Path, arguments: &[&str]) -> Output {
    Command::cargo_bin("gitrex")
        .expect("gitrex binary")
        .current_dir(repository)
        .args(arguments)
        .output()
        .expect("run gitrex")
}

fn json_command(repository: &Path, arguments: &[&str]) -> Value {
    let output = gitrex(repository, arguments);
    assert!(
        output.status.success(),
        "gitrex {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON response")
}

#[test]
fn inspect_returns_versioned_repository_context() {
    let repository = repository();
    let head = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    git(
        repository.path(),
        &["remote", "add", "origin", "http://127.0.0.1:1/never-fetch"],
    );
    git(
        repository.path(),
        &["update-ref", "refs/remotes/origin/main", head.as_str()],
    );
    git(
        repository.path(),
        &["config", "branch.main.remote", "origin"],
    );
    git(
        repository.path(),
        &["config", "branch.main.merge", "refs/heads/main"],
    );
    let output = gitrex(
        repository.path(),
        &["inspect", "--scope", "minimal", "--format", "json"],
    );
    assert!(
        output.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let response: Value = serde_json::from_slice(&output.stdout).expect("valid JSON envelope");
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["operation"], "inspect");
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["scope"], "minimal");
    assert_eq!(
        response["data"]["repository"]["root"],
        repository
            .path()
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(response["data"]["head"]["branch"], "main");
    assert_eq!(response["data"]["head"]["commit"], head);
    assert_eq!(response["data"]["upstream"]["reference"], "origin/main");
    assert_eq!(response["data"]["upstream"]["ahead"], 0);
    assert_eq!(response["data"]["upstream"]["behind"], 0);
    assert_eq!(response["data"]["working_tree"]["clean"], true);
    assert!(response["data"].get("branches").is_none());
    assert!(response["data"].get("history").is_none());
}

#[test]
fn inspect_handles_an_unborn_repository() {
    let directory = tempfile::tempdir().expect("temporary repository directory");
    git(
        directory.path(),
        &["init", "--quiet", "--initial-branch=main"],
    );

    let response = json_command(
        directory.path(),
        &["inspect", "--scope", "full", "--format", "json"],
    );
    assert_eq!(response["data"]["head"]["branch"], "main");
    assert!(response["data"]["head"]["commit"].is_null());
    assert_eq!(response["data"]["head"]["detached"], false);
    assert_eq!(response["data"]["working_tree"]["clean"], true);
    assert_eq!(response["data"]["history"]["count"], 0);
    assert_eq!(
        response["data"]["history"]["commits"],
        serde_json::json!([])
    );
}

#[test]
fn inspect_scopes_include_their_documented_context_groups() {
    let repository = repository();
    for name in ["one", "two"] {
        std::fs::write(repository.path().join(format!("history-{name}.txt")), name)
            .expect("write history fixture");
        git(
            repository.path(),
            &["add", "--", &format!("history-{name}.txt")],
        );
        git(
            repository.path(),
            &["commit", "--quiet", "-m", &format!("history {name}")],
        );
    }
    git(repository.path(), &["branch", "side"]);
    std::fs::write(repository.path().join("README.md"), "staged\n").expect("stage fixture edit");
    git(repository.path(), &["add", "--", "README.md"]);
    std::fs::write(repository.path().join("README.md"), "unstaged\n")
        .expect("unstage fixture edit");
    std::fs::write(repository.path().join("new.txt"), "untracked\n")
        .expect("write untracked fixture");
    std::fs::write(
        repository.path().join("another-new.txt"),
        "also untracked\n",
    )
    .expect("write second untracked fixture");

    let minimal = json_command(
        repository.path(),
        &["inspect", "--scope", "minimal", "--format", "json"],
    );
    assert_eq!(minimal["data"]["working_tree"]["staged"]["count"], 1);
    assert!(minimal["data"]["working_tree"]["staged"]
        .get("paths")
        .is_none());
    assert!(minimal["data"].get("branches").is_none());
    assert!(minimal["data"].get("history").is_none());

    let change = json_command(
        repository.path(),
        &["inspect", "--scope", "change", "--format", "json"],
    );
    assert_eq!(
        change["data"]["working_tree"]["staged"]["paths"][0]["path"],
        "README.md"
    );
    assert_eq!(
        change["data"]["working_tree"]["unstaged"]["paths"][0]["path"],
        "README.md"
    );
    assert_eq!(
        change["data"]["working_tree"]["untracked"]["paths"][0],
        "another-new.txt"
    );
    assert!(change["data"].get("branches").is_none());
    assert!(change["data"].get("history").is_none());

    let branches = json_command(
        repository.path(),
        &["inspect", "--scope", "branches", "--format", "json"],
    );
    assert!(branches["data"]["branches"]["count"].as_u64().unwrap() >= 1);
    assert!(branches["data"]["working_tree"]["staged"]
        .get("paths")
        .is_none());
    assert!(branches["data"].get("history").is_none());

    let full = json_command(
        repository.path(),
        &[
            "inspect",
            "--format",
            "json",
            "--history-limit",
            "1",
            "--max-branches",
            "1",
            "--max-paths",
            "1",
        ],
    );
    assert_eq!(full["data"]["scope"], "full");
    assert_eq!(
        full["data"]["history"]["commits"].as_array().unwrap().len(),
        1
    );
    assert_eq!(full["data"]["history"]["count"], 3);
    assert_eq!(full["data"]["history"]["truncated"], true);
    assert_eq!(full["data"]["branches"]["count"], 2);
    assert_eq!(
        full["data"]["branches"]["branches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(full["data"]["branches"]["truncated"], true);
    assert!(full["data"]["branches"].get("branches").is_some());
    assert_eq!(full["data"]["working_tree"]["untracked"]["count"], 2);
    assert_eq!(
        full["data"]["working_tree"]["untracked"]["paths"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(full["data"]["working_tree"]["untracked"]["truncated"], true);
}

#[test]
fn inspect_reports_conflict_paths() {
    let repository = repository();
    let base = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    git(repository.path(), &["checkout", "--quiet", "-b", "side"]);
    std::fs::write(repository.path().join("README.md"), "side\n")
        .expect("write side branch content");
    git(
        repository.path(),
        &["commit", "--quiet", "-am", "side change"],
    );
    git(repository.path(), &["checkout", "--quiet", "main"]);
    std::fs::write(repository.path().join("README.md"), "main\n")
        .expect("write main branch content");
    git(
        repository.path(),
        &["commit", "--quiet", "-am", "main change"],
    );
    let merge = ProcessCommand::new("git")
        .arg("-C")
        .arg(repository.path())
        .args(["merge", "side"])
        .output()
        .expect("run conflicting merge");
    assert!(
        !merge.status.success(),
        "fixture should leave a merge conflict"
    );

    let response = json_command(
        repository.path(),
        &["inspect", "--scope", "change", "--format", "json"],
    );
    assert_eq!(response["data"]["conflicts"]["has_conflicts"], true);
    assert_eq!(
        response["data"]["conflicts"]["paths"][0]["path"],
        "README.md"
    );
    assert_ne!(response["data"]["head"]["commit"], base);
}

#[test]
fn diff_supports_worktree_staged_base_and_ref_modes_with_explicit_bounds() {
    let repository = repository();
    std::fs::write(repository.path().join("README.md"), "working tree change\n")
        .expect("write worktree change");
    let worktree = json_command(
        repository.path(),
        &["diff", "--format", "json", "--include-patch"],
    );
    assert_eq!(worktree["operation"], "diff");
    assert_eq!(worktree["data"]["changed_file_count"], 1);
    assert_eq!(worktree["data"]["changed_files"][0]["path"], "README.md");
    assert_eq!(worktree["data"]["additions"], 1);
    assert_eq!(worktree["data"]["deletions"], 1);
    assert_eq!(worktree["data"]["changed_files"][0]["additions"], 1);
    assert!(worktree["data"]["patch"]["text"]
        .as_str()
        .unwrap()
        .contains("working tree change"));

    let text = gitrex(repository.path(), &["diff", "--max-patch-bytes", "8"]);
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("truncated"));

    git(repository.path(), &["add", "--", "README.md"]);
    std::fs::write(repository.path().join("second.txt"), "second staged file\n")
        .expect("write second staged file");
    git(repository.path(), &["add", "--", "second.txt"]);
    let staged = json_command(
        repository.path(),
        &["diff", "--staged", "--format", "json", "--max-paths", "1"],
    );
    assert_eq!(staged["data"]["changed_files"][0]["status"], "M");
    assert_eq!(staged["data"]["changed_file_count"], 2);
    assert_eq!(staged["data"]["changed_files"].as_array().unwrap().len(), 1);
    assert_eq!(staged["data"]["changed_files_truncated"], true);

    let base = json_command(
        repository.path(),
        &[
            "diff",
            "--base",
            "HEAD",
            "--format",
            "json",
            "--max-patch-bytes",
            "8",
            "--include-patch",
        ],
    );
    assert_eq!(base["data"]["changed_file_count"], 2);
    assert_eq!(base["data"]["patch"]["truncated"], true);
    assert!(base["data"]["patch"]["total_bytes"].as_u64().unwrap() > 8);
    assert!(base["data"]["patch"]["text"].as_str().unwrap().len() <= 8);

    git(repository.path(), &["checkout", "--quiet", "-b", "feature"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "feature change"],
    );
    let refs = json_command(
        repository.path(),
        &[
            "diff", "--from", "main", "--to", "feature", "--format", "json",
        ],
    );
    assert_eq!(refs["data"]["left_commit"].as_str().unwrap().len(), 40);
    assert_eq!(refs["data"]["right_commit"].as_str().unwrap().len(), 40);
    assert_eq!(refs["data"]["changed_file_count"], 2);
}

#[test]
fn show_returns_commit_metadata_and_bounded_diff() {
    let repository = repository();
    let root_commit = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    std::fs::write(repository.path().join("README.md"), "detail commit body\n")
        .expect("write commit change");
    git(repository.path(), &["add", "--", "README.md"]);
    git(
        repository.path(),
        &[
            "commit",
            "--quiet",
            "-m",
            "detail commit",
            "-m",
            "body line",
        ],
    );

    let response = json_command(
        repository.path(),
        &[
            "show",
            "HEAD",
            "--format",
            "json",
            "--include-patch",
            "--max-patch-bytes",
            "16",
        ],
    );
    assert_eq!(response["operation"], "show");
    assert_eq!(response["data"]["commit"]["subject"], "detail commit");
    assert_eq!(response["data"]["commit"]["body"], "body line");
    assert_eq!(response["data"]["commit"]["author"], "GitRex Test");
    assert_eq!(
        response["data"]["commit"]["parents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(response["data"]["diff"]["patch"]["truncated"], true);
    assert!(
        response["data"]["diff"]["changed_file_count"]
            .as_u64()
            .unwrap()
            >= 1
    );

    let root = json_command(
        repository.path(),
        &[
            "show",
            root_commit.as_str(),
            "--format",
            "json",
            "--include-patch",
        ],
    );
    assert!(root["data"]["diff"]["patch"]["text"]
        .as_str()
        .unwrap()
        .contains("initial"));
    assert_eq!(root["data"]["diff"]["changed_files"][0]["status"], "A");
}

#[test]
fn compare_reports_merge_base_divergence_and_unique_commits() {
    let repository = repository();
    let base = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    git(repository.path(), &["checkout", "--quiet", "-b", "left"]);
    std::fs::write(repository.path().join("left.txt"), "left\n").expect("write left branch file");
    git(repository.path(), &["add", "--", "left.txt"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "left change"],
    );
    std::fs::write(repository.path().join("left-second.txt"), "left second\n")
        .expect("write second left branch file");
    git(repository.path(), &["add", "--", "left-second.txt"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "left second change"],
    );
    git(
        repository.path(),
        &["checkout", "--quiet", "-b", "right", base.as_str()],
    );
    std::fs::write(repository.path().join("right.txt"), "right\n")
        .expect("write right branch file");
    git(repository.path(), &["add", "--", "right.txt"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "right change"],
    );
    std::fs::write(repository.path().join("right-second.txt"), "right second\n")
        .expect("write second right branch file");
    git(repository.path(), &["add", "--", "right-second.txt"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "right second change"],
    );

    let response = json_command(
        repository.path(),
        &[
            "compare",
            "left",
            "right",
            "--format",
            "json",
            "--max-commits",
            "1",
        ],
    );
    assert_eq!(response["operation"], "compare");
    assert_eq!(response["data"]["merge_bases"][0], base);
    assert_eq!(response["data"]["ahead"], 2);
    assert_eq!(response["data"]["behind"], 2);
    assert_eq!(response["data"]["left_only_commit_count"], 2);
    assert_eq!(
        response["data"]["left_only_commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(response["data"]["left_only_commits_truncated"], true);
    assert_eq!(response["data"]["right_only_commit_count"], 2);
    assert_eq!(
        response["data"]["right_only_commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(response["data"]["right_only_commits_truncated"], true);
    assert_eq!(
        response["data"]["left_only_commits"][0]["subject"],
        "left second change"
    );
    assert_eq!(
        response["data"]["right_only_commits"][0]["subject"],
        "right second change"
    );
    assert_eq!(response["data"]["diff"]["changed_file_count"], 4);
}

#[test]
fn change_context_combines_committed_and_uncommitted_state() {
    let repository = repository();
    let base = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    std::fs::write(repository.path().join("README.md"), "committed change\n")
        .expect("write committed change");
    git(repository.path(), &["add", "--", "README.md"]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "context commit"],
    );
    std::fs::write(repository.path().join("README.md"), "unstaged change\n")
        .expect("write unstaged change");
    std::fs::write(repository.path().join("staged.txt"), "staged\n").expect("write staged file");
    git(repository.path(), &["add", "--", "staged.txt"]);
    std::fs::write(repository.path().join("new.txt"), "untracked\n").expect("write untracked file");

    let response = json_command(
        repository.path(),
        &[
            "change-context",
            "--base",
            base.as_str(),
            "--format",
            "json",
        ],
    );
    assert_eq!(response["operation"], "change-context");
    assert_eq!(response["data"]["base"]["commit"], base);
    assert_eq!(response["data"]["commits"]["count"], 1);
    assert_eq!(
        response["data"]["commits"]["commits"][0]["subject"],
        "context commit"
    );
    assert_eq!(
        response["data"]["diff"]["changed_files"][0]["path"],
        "README.md"
    );
    assert_eq!(
        response["data"]["working_tree"]["staged"]["paths"][0]["path"],
        "staged.txt"
    );
    assert_eq!(
        response["data"]["working_tree"]["unstaged"]["paths"][0]["path"],
        "README.md"
    );
    assert_eq!(
        response["data"]["working_tree"]["untracked"]["paths"][0],
        "new.txt"
    );
    assert_eq!(response["data"]["conflicts"]["has_conflicts"], false);
}

#[test]
fn invalid_context_reference_uses_the_stable_json_error_envelope() {
    let repository = repository();
    let output = gitrex(
        repository.path(),
        &["compare", "missing-left", "HEAD", "--format", "json"],
    );
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).expect("JSON error envelope");
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["operation"], "compare");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "REFERENCE_NOT_FOUND");
}

#[test]
fn capabilities_classify_new_context_operations_as_read_only() {
    let repository = repository();
    let response = json_command(repository.path(), &["capabilities", "--format", "json"]);
    for name in ["inspect", "diff", "show", "compare", "change-context"] {
        let operation = response["data"]["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["name"] == name)
            .unwrap_or_else(|| panic!("missing capability {name}"));
        assert_eq!(operation["effects"], serde_json::json!(["read_only"]));
        assert_eq!(
            operation["output_formats"],
            serde_json::json!(["text", "json"])
        );
    }
}
