use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

#[test]
fn cli_covers_core_git_operations_against_real_repos() {
    let temp = TempDir::new().unwrap();
    let origin = temp.path().join("origin.git");
    let worktree = temp.path().join("worktree");
    let clone_dir = temp.path().join("clone");

    run_git(temp.path(), &["init", "--bare", origin.file_name().unwrap().to_str().unwrap()]);
    run_git(
        temp.path(),
        &[
            "clone",
            origin.file_name().unwrap().to_str().unwrap(),
            worktree.file_name().unwrap().to_str().unwrap(),
        ],
    );
    configure_repo(&worktree);
    write_file(&worktree, "README.md", "gitrex\n");
    run_git(&worktree, &["add", "README.md"]);
    run_git(&worktree, &["commit", "-m", "initial commit"]);
    run_git(&worktree, &["branch", "-M", "main"]);
    run_git(&worktree, &["push", "-u", "origin", "main"]);
    run_git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    assert_gitrex(&worktree, &["status"])
        .success()
        .stdout(predicates::str::contains("branch: main"))
        .stdout(predicates::str::contains("upstream: origin/main"))
        .stdout(predicates::str::contains("working tree: clean"));

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("* main -> origin/main"));

    assert_gitrex(&worktree, &["log", "--limit", "1"])
        .success()
        .stdout(predicates::str::contains("initial commit"));

    assert_gitrex(&worktree, &["create-branch", "feature/login", "--from", "main"])
        .success()
        .stdout(predicates::str::contains("created feature/login from main"));

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("feature/login"));

    assert_gitrex(&worktree, &["checkout", "main"])
        .success()
        .stdout(predicates::str::contains("checked out main"));

    assert_gitrex(&worktree, &["switch", "feature/login"])
        .success()
        .stdout(predicates::str::contains("switched to feature/login"));

    assert_gitrex(temp.path(), &["clone", origin.to_str().unwrap(), clone_dir.to_str().unwrap()])
        .success()
        .stdout(predicates::str::contains("clone complete"));
    assert!(clone_dir.join(".git").exists());
}

#[test]
fn cli_can_push_and_pull_with_a_real_remote() {
    let temp = TempDir::new().unwrap();
    let origin = temp.path().join("origin.git");
    let worktree = temp.path().join("worktree");
    let collaborator = temp.path().join("collaborator");

    run_git(temp.path(), &["init", "--bare", origin.file_name().unwrap().to_str().unwrap()]);
    run_git(
        temp.path(),
        &[
            "clone",
            origin.file_name().unwrap().to_str().unwrap(),
            worktree.file_name().unwrap().to_str().unwrap(),
        ],
    );
    configure_repo(&worktree);
    write_file(&worktree, "README.md", "base\n");
    run_git(&worktree, &["add", "README.md"]);
    run_git(&worktree, &["commit", "-m", "base"]);
    run_git(&worktree, &["branch", "-M", "main"]);
    run_git(&worktree, &["push", "-u", "origin", "main"]);
    run_git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    run_git(
        temp.path(),
        &[
            "clone",
            origin.file_name().unwrap().to_str().unwrap(),
            collaborator.file_name().unwrap().to_str().unwrap(),
        ],
    );
    configure_repo(&collaborator);
    write_file(&collaborator, "README.md", "base\ncollaborator\n");
    run_git(&collaborator, &["add", "README.md"]);
    run_git(&collaborator, &["commit", "-m", "remote update"]);
    run_git(&collaborator, &["push", "origin", "main"]);

    assert_gitrex(&worktree, &["pull", "origin", "main"])
        .success()
        .stdout(predicates::str::contains("pull complete"));

    write_file(&worktree, "feature.txt", "feature\n");
    run_git(&worktree, &["add", "feature.txt"]);
    run_git(&worktree, &["commit", "-m", "feature work"]);
    run_git(&worktree, &["checkout", "-b", "feature/login"]);
    assert_gitrex(&worktree, &["push", "origin", "feature/login"])
        .success()
        .stdout(predicates::str::contains("push complete"));

    let remote_ref = run_git_output(
        &origin,
        &["rev-parse", "--verify", "refs/heads/feature/login"],
    );
    assert!(!remote_ref.trim().is_empty());
}

fn assert_gitrex(dir: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = AssertCommand::cargo_bin("gitrex").unwrap();
    cmd.current_dir(dir);
    cmd.args(args);
    cmd.assert()
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed in {}", args, dir.display());
}

fn run_git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed in {}",
        args,
        dir.display()
    );
    String::from_utf8(output.stdout).unwrap()
}

fn configure_repo(dir: &Path) {
    run_git(dir, &["config", "user.name", "Gitrex Test"]);
    run_git(dir, &["config", "user.email", "gitrex@example.com"]);
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    fs::write(PathBuf::from(dir).join(name), contents).unwrap();
}
