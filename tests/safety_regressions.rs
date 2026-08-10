use std::fs;

use tempfile::TempDir;

use gitrex::{
    git::GitClient,
    test_support::{
        checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
        current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
        CurrentDirGuard,
    },
};

#[test]
fn checkout_refuses_to_overwrite_dirty_worktree() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), "README.md", "base\n");
    commit_all(&repo, "base");

    create_branch(&repo, "feature/login", "HEAD");
    checkout_branch(&repo, "feature/login");
    write_file(temp.path(), "README.md", "feature\n");
    commit_all(&repo, "feature");
    checkout_branch(&repo, "main");
    write_file(temp.path(), "README.md", "dirty\n");

    let client = GitClient::from_path(temp.path());
    assert!(client.checkout("feature/login").is_err());

    assert_eq!(
        repo.head().unwrap().shorthand(),
        Some("main"),
        "HEAD must not move when the checkout preflight fails"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("README.md")).unwrap(),
        "dirty\n"
    );
}

#[test]
fn pull_is_a_noop_when_local_and_remote_are_equal() {
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
    let before = worktree_repo.head().unwrap().target().unwrap();

    GitClient::from_path(&worktree)
        .pull(Some("origin"), Some("main"))
        .unwrap();

    assert_eq!(worktree_repo.head().unwrap().target(), Some(before));
}

#[test]
fn pull_is_a_noop_when_local_is_ahead() {
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

    write_file(&worktree, "local.txt", "local\n");
    commit_all(&worktree_repo, "local work");
    let before = worktree_repo.head().unwrap().target().unwrap();

    GitClient::from_path(&worktree)
        .pull(Some("origin"), Some("main"))
        .unwrap();

    assert_eq!(worktree_repo.head().unwrap().target(), Some(before));
}

#[test]
fn pull_rejects_diverged_histories_without_moving_local_head() {
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

    write_file(&worktree, "local.txt", "local\n");
    commit_all(&worktree_repo, "local work");
    let local_head = worktree_repo.head().unwrap().target().unwrap();

    write_file(&collaborator, "remote.txt", "remote\n");
    commit_all(&collaborator_repo, "remote work");
    push_branch(&collaborator_repo, "origin", "main");

    let error = GitClient::from_path(&worktree)
        .pull(Some("origin"), Some("main"))
        .unwrap_err();

    assert!(error.to_string().contains("diverged"));
    assert_eq!(worktree_repo.head().unwrap().target(), Some(local_head));
}

#[test]
fn snapshot_does_not_fetch_remote_refs() {
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

    let tracked_before = worktree_repo
        .find_reference("refs/remotes/origin/main")
        .unwrap()
        .target()
        .unwrap();

    write_file(&collaborator, "README.md", "remote update\n");
    commit_all(&collaborator_repo, "remote update");
    push_branch(&collaborator_repo, "origin", "main");

    GitClient::from_path(&worktree).snapshot().unwrap();

    let tracked_after = worktree_repo
        .find_reference("refs/remotes/origin/main")
        .unwrap()
        .target()
        .unwrap();
    assert_eq!(tracked_after, tracked_before);
}

#[test]
fn new_client_stays_bound_to_the_repository_where_it_was_created() {
    let _guard = current_dir_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");

    let first_repo = init_repo(&first, "main");
    configure_user(&first_repo);
    write_file(&first, "README.md", "first\n");
    commit_all(&first_repo, "first");

    let second_repo = init_repo(&second, "secondary");
    configure_user(&second_repo);
    write_file(&second, "README.md", "second\n");
    commit_all(&second_repo, "second");

    let client = {
        let _cwd = CurrentDirGuard::push(&first);
        GitClient::new()
    };
    let _cwd = CurrentDirGuard::push(&second);

    assert_eq!(client.status().unwrap().branch_name, "main");
}
