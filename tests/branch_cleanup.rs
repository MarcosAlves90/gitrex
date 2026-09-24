mod support;

use std::{path::Path, process::Command};

use support::{
    checkout_branch, commit_all, configure_user, create_branch, init_repo, set_upstream,
    write_file, TestRepo,
};
use tempfile::TempDir;

fn initialized_repo() -> (TempDir, TestRepo) {
    let temp = tempfile::tempdir().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), "base.txt", "base\n");
    commit_all(&repo, "initial commit");
    (temp, repo)
}

fn run_gitrex(repository: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gitrex"))
        .current_dir(repository)
        .args(arguments)
        .output()
        .unwrap()
}

fn output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn cleanup_preview_lists_merged_candidates_without_deleting_them() {
    let (temp, repo) = initialized_repo();
    create_branch(&repo, "feature/merged", "HEAD");
    create_branch(&repo, "feature/unmerged", "HEAD");
    checkout_branch(&repo, "feature/unmerged");
    write_file(temp.path(), "unmerged.txt", "unmerged change\n");
    commit_all(&repo, "unmerged change");
    checkout_branch(&repo, "main");

    let output = run_gitrex(temp.path(), &["cleanup"]);
    let text = output_text(&output);

    assert!(output.status.success(), "cleanup preview failed: {text}");
    assert!(text.contains("feature/merged"), "missing candidate: {text}");
    assert!(
        !text.contains("feature/unmerged"),
        "unmerged branch appeared in preview: {text}"
    );
    repo.find_branch("main").unwrap();
    repo.find_branch("feature/merged").unwrap();
    repo.find_branch("feature/unmerged").unwrap();
}

#[test]
fn cleanup_accepts_multiple_selected_remotes_and_applies_exact_exclusions() {
    let (temp, repo) = initialized_repo();
    create_branch(&repo, "release", "main");
    checkout_branch(&repo, "release");
    write_file(temp.path(), "release.txt", "release change\n");
    let release_oid = commit_all(&repo, "release change");

    for remote in ["origin", "upstream", "third"] {
        repo.remote(remote, "https://example.invalid/gitrex.git")
            .unwrap();
        let reference = format!("refs/remotes/{remote}/release");
        repo.reference(
            &reference,
            release_oid.clone(),
            false,
            "fixture remote-tracking ref",
        )
        .unwrap();
    }

    create_branch(&repo, "feature/origin", "release");
    create_branch(&repo, "feature/upstream", "release");
    create_branch(&repo, "feature/third", "release");
    create_branch(&repo, "feature/excluded", "release");
    create_branch(&repo, "feature/untracked", "release");
    set_upstream(&repo, "feature/origin", "origin/release");
    set_upstream(&repo, "feature/upstream", "upstream/release");
    set_upstream(&repo, "feature/third", "third/release");
    set_upstream(&repo, "feature/excluded", "origin/release");
    checkout_branch(&repo, "main");

    let output = run_gitrex(
        temp.path(),
        &[
            "cleanup",
            "--base",
            "release",
            "--remote",
            "origin",
            "--remote",
            "upstream",
            "--exclude",
            "feature/excluded",
        ],
    );
    let text = output_text(&output);

    assert!(output.status.success(), "filtered preview failed: {text}");
    assert!(
        text.contains("feature/origin"),
        "origin candidate was not listed: {text}"
    );
    assert!(
        text.contains("feature/upstream"),
        "upstream candidate was not listed: {text}"
    );
    assert!(
        !text.contains("feature/third"),
        "branch from an unselected remote appeared in preview: {text}"
    );
    assert!(
        !text.contains("feature/excluded"),
        "exactly excluded branch appeared in preview: {text}"
    );
    assert!(
        !text.contains("feature/untracked"),
        "branch without the selected remote upstream appeared: {text}"
    );
}

#[test]
fn cleanup_yes_deletes_only_merged_branches_and_protects_current_and_base() {
    let (temp, repo) = initialized_repo();
    create_branch(&repo, "release", "HEAD");
    create_branch(&repo, "feature/merged", "HEAD");
    create_branch(&repo, "feature/unmerged", "HEAD");
    checkout_branch(&repo, "feature/unmerged");
    write_file(temp.path(), "unmerged.txt", "unmerged change\n");
    commit_all(&repo, "unmerged change");
    checkout_branch(&repo, "main");

    let output = run_gitrex(temp.path(), &["cleanup", "--base", "release", "--yes"]);
    let text = output_text(&output);

    assert!(output.status.success(), "automatic cleanup failed: {text}");
    assert!(
        repo.find_branch("feature/merged").is_err(),
        "merged candidate still exists after --yes: {text}"
    );
    repo.find_branch("main").unwrap();
    repo.find_branch("release").unwrap();
    repo.find_branch("feature/unmerged").unwrap();
}

#[test]
fn cleanup_rejects_an_invalid_base_without_deleting_branches() {
    let (temp, repo) = initialized_repo();
    create_branch(&repo, "feature/merged", "HEAD");

    let output = run_gitrex(temp.path(), &["cleanup", "--base", "missing-base", "--yes"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "invalid base unexpectedly succeeded"
    );
    assert!(
        text.to_lowercase().contains("base"),
        "invalid base was not diagnosed: {text}"
    );
    repo.find_branch("feature/merged").unwrap();
    repo.find_branch("main").unwrap();
}
