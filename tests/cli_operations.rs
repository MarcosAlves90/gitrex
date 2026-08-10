use assert_cmd::Command as AssertCommand;
use predicates::prelude::PredicateBooleanExt;
use tempfile::TempDir;

use gitrex::test_support::{
    checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
    current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
    CurrentDirGuard,
};

#[test]
fn cli_covers_core_git_operations_against_real_repos() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let temp = TempDir::new().unwrap();
    let seed = temp.path().join("seed");
    let origin = temp.path().join("origin.git");
    let worktree = temp.path().join("worktree");
    let clone_dir = temp.path().join("clone");

    let seed_repo = init_repo(&seed, "main");
    configure_user(&seed_repo);
    write_file(&seed, "README.md", "gitrex\n");
    commit_all(&seed_repo, "initial commit");

    let origin_repo = clone_bare_repo(&seed, &origin);
    set_remote_head(&origin_repo, "refs/heads/main");

    let worktree_repo = clone_repo(&origin, &worktree);
    configure_user(&worktree_repo);
    set_upstream(&worktree_repo, "main", "origin/main");

    let _cwd = CurrentDirGuard::push(&worktree);

    assert_gitrex(&worktree, &["status"])
        .success()
        .stdout(predicates::str::contains("branch: main"))
        .stdout(predicates::str::contains("upstream: origin/main"))
        .stdout(predicates::str::contains("working tree: clean"));

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("remote branches:"))
        .stdout(predicates::str::contains("local branches:"))
        .stdout(predicates::str::contains("* main [synced: origin/main]"));

    assert_gitrex(&worktree, &["log", "--limit", "1"])
        .success()
        .stdout(predicates::str::contains("initial commit"));

    assert_gitrex(
        &worktree,
        &["create-branch", "feature/login", "--from", "main"],
    )
    .success()
    .stdout(predicates::str::contains("created feature/login from main"));

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("feature/login [local-only]"));

    assert_gitrex(&worktree, &["checkout", "main"])
        .success()
        .stdout(predicates::str::contains("checked out main"));

    assert_gitrex(&worktree, &["switch", "feature/login"])
        .success()
        .stdout(predicates::str::contains("switched to feature/login"));

    let _cwd = CurrentDirGuard::push(temp.path());
    assert_gitrex(
        temp.path(),
        &[
            "clone",
            origin.to_str().unwrap(),
            clone_dir.to_str().unwrap(),
        ],
    )
    .success()
    .stdout(predicates::str::contains("clone complete"));
    assert!(clone_dir.join(".git").exists());
}

#[test]
fn cli_branch_view_separates_multiple_remotes() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let temp = TempDir::new().unwrap();
    let seed = temp.path().join("seed");
    let origin = temp.path().join("origin.git");
    let upstream = temp.path().join("upstream.git");
    let worktree = temp.path().join("worktree");

    let seed_repo = init_repo(&seed, "main");
    configure_user(&seed_repo);
    write_file(&seed, "README.md", "base\n");
    commit_all(&seed_repo, "base");

    let origin_repo = clone_bare_repo(&seed, &origin);
    set_remote_head(&origin_repo, "refs/heads/main");
    let upstream_repo = clone_bare_repo(&seed, &upstream);
    set_remote_head(&upstream_repo, "refs/heads/main");

    let worktree_repo = clone_repo(&origin, &worktree);
    configure_user(&worktree_repo);
    set_upstream(&worktree_repo, "main", "origin/main");
    worktree_repo
        .remote("upstream", upstream.to_str().unwrap())
        .unwrap();
    worktree_repo
        .find_remote("upstream")
        .unwrap()
        .fetch(&["main"], None, None)
        .unwrap();

    let _cwd = CurrentDirGuard::push(&worktree);

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("remote branches:"))
        .stdout(predicates::str::contains("origin"))
        .stdout(predicates::str::contains("upstream"))
        .stdout(predicates::str::contains("local branches:"))
        .stdout(predicates::str::contains(
            "* main [synced: origin/main, upstream/main]",
        ));
}

#[test]
fn cli_can_push_and_pull_with_a_real_remote() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let temp = TempDir::new().unwrap();
    let seed = temp.path().join("seed");
    let origin = temp.path().join("origin.git");
    let worktree = temp.path().join("worktree");
    let collaborator = temp.path().join("collaborator");

    let seed_repo = init_repo(&seed, "main");
    configure_user(&seed_repo);
    write_file(&seed, "README.md", "base\n");
    commit_all(&seed_repo, "base");

    let origin_repo = clone_bare_repo(&seed, &origin);
    set_remote_head(&origin_repo, "refs/heads/main");

    let worktree_repo = clone_repo(&origin, &worktree);
    configure_user(&worktree_repo);
    set_upstream(&worktree_repo, "main", "origin/main");

    let collaborator_repo = clone_repo(&origin, &collaborator);
    configure_user(&collaborator_repo);

    write_file(&collaborator, "README.md", "base\ncollaborator\n");
    commit_all(&collaborator_repo, "remote update");
    push_branch(&collaborator_repo, "origin", "main");

    let _cwd = CurrentDirGuard::push(&worktree);

    assert_gitrex(&worktree, &["pull", "origin", "main"])
        .success()
        .stdout(predicates::str::contains("pull complete"));

    write_file(&worktree, "feature.txt", "feature\n");
    commit_all(&worktree_repo, "feature work");
    create_branch(&worktree_repo, "feature/login", "HEAD");
    checkout_branch(&worktree_repo, "feature/login");

    assert_gitrex(&worktree, &["push", "origin", "feature/login"])
        .success()
        .stdout(predicates::str::contains("push complete"));

    let origin_repo = git2::Repository::open_bare(&origin).unwrap();
    assert!(origin_repo
        .find_reference("refs/heads/feature/login")
        .is_ok());
}

#[test]
fn cli_branch_refresh_removes_deleted_remote_refs() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let temp = TempDir::new().unwrap();
    let seed = temp.path().join("seed");
    let origin = temp.path().join("origin.git");
    let worktree = temp.path().join("worktree");

    let seed_repo = init_repo(&seed, "main");
    configure_user(&seed_repo);
    write_file(&seed, "README.md", "base\n");
    commit_all(&seed_repo, "base");

    let origin_repo = clone_bare_repo(&seed, &origin);
    set_remote_head(&origin_repo, "refs/heads/main");

    let worktree_repo = clone_repo(&origin, &worktree);
    configure_user(&worktree_repo);
    set_upstream(&worktree_repo, "main", "origin/main");

    write_file(&worktree, "feature.txt", "feature\n");
    commit_all(&worktree_repo, "feature work");
    create_branch(&worktree_repo, "feature/login", "HEAD");
    push_branch(&worktree_repo, "origin", "feature/login");

    let _cwd = CurrentDirGuard::push(&worktree);

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("origin/feature/login"));

    let mut remote_feature = origin_repo
        .find_reference("refs/heads/feature/login")
        .unwrap();
    remote_feature.delete().unwrap();

    assert_gitrex(&worktree, &["fetch", "origin"])
        .success()
        .stdout(predicates::str::contains("fetch complete"));

    assert_gitrex(&worktree, &["branch"])
        .success()
        .stdout(predicates::str::contains("remote branches:"))
        .stdout(predicates::str::contains("local branches:"))
        .stdout(predicates::str::contains("feature/login [local-only]"))
        .stdout(predicates::str::contains("origin/feature/login").not());
}

fn assert_gitrex(dir: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = AssertCommand::cargo_bin("gitrex").unwrap();
    cmd.current_dir(dir);
    cmd.args(args);
    cmd.assert()
}
