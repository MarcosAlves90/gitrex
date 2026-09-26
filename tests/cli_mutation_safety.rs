use assert_cmd::Command as AssertCommand;
use gitrex::{
    app::operations::execute_mutation,
    domain::{
        MutationRequest, OperationExecution, OperationPreconditions, OperationResetMode,
        VerificationStatus,
    },
    git::GitClient,
};
use serde_json::Value;
use std::process::Output;
use std::{fs, path::Path};
use tempfile::TempDir;

mod support;

use support::{
    clone_bare_repo, clone_repo, commit_all, configure_user, create_branch, init_repo, push_branch,
    set_remote_head, set_upstream, write_file,
};

#[test]
fn dry_run_plans_switch_and_cleanup_without_mutation() {
    let temp = TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), "README.md", "gitrex\n");
    let head = commit_all(&repo, "initial commit");
    create_branch(&repo, "merged", &head);

    let switch = run_gitrex(
        temp.path(),
        &["switch", "merged", "--dry-run", "--format", "json"],
    );
    assert!(
        switch.status.success(),
        "{}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let switch = json_response(&switch);
    assert_eq!(switch["data"]["operation"], "switch");
    assert_eq!(switch["data"]["risk_class"], "local_mutation");
    assert_eq!(switch["data"]["planning_source"], "gitrex_analysis");

    let cleanup = run_gitrex(
        temp.path(),
        &["cleanup", "--base", "main", "--dry-run", "--format", "json"],
    );
    assert!(
        cleanup.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    let cleanup = json_response(&cleanup);
    assert_eq!(cleanup["data"]["operation"], "cleanup");
    assert_eq!(cleanup["data"]["risk_class"], "destructive");
    assert!(cleanup["data"]["expected_local_effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|effect| effect["target"] == "refs/heads/merged"));

    assert_eq!(
        repo.head().unwrap().target().as_deref(),
        Some(head.as_str())
    );
    repo.find_branch("merged").unwrap();

    let switched = run_gitrex(temp.path(), &["switch", "merged", "--format", "json"]);
    assert!(
        switched.status.success(),
        "{}",
        String::from_utf8_lossy(&switched.stderr)
    );
    let switched = json_response(&switched);
    assert_eq!(switched["data"]["verification"]["status"], "verified");
    assert_eq!(switched["data"]["after"]["branch"], "merged");

    let returned = run_gitrex(temp.path(), &["switch", "main", "--format", "json"]);
    assert!(
        returned.status.success(),
        "{}",
        String::from_utf8_lossy(&returned.stderr)
    );
    let returned = json_response(&returned);
    assert_eq!(returned["data"]["verification"]["status"], "verified");

    let cleanup = run_gitrex(
        temp.path(),
        &["cleanup", "--base", "main", "--yes", "--format", "json"],
    );
    assert!(
        cleanup.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    let cleanup = json_response(&cleanup);
    assert_eq!(cleanup["data"]["verification"]["status"], "verified");
    assert_eq!(cleanup["data"]["state_changed"], true);
    assert!(repo.find_branch("merged").is_err());
}

#[test]
fn dry_run_plans_fetch_pull_and_push_without_local_or_remote_mutation() {
    let temp = TempDir::new().unwrap();
    let seed_path = temp.path().join("seed");
    let origin_path = temp.path().join("origin.git");
    let worktree_path = temp.path().join("worktree");
    let seed = init_repo(&seed_path, "main");
    configure_user(&seed);
    write_file(&seed_path, "README.md", "initial\n");
    let initial = commit_all(&seed, "initial commit");
    let origin = clone_bare_repo(&seed_path, &origin_path);
    set_remote_head(&origin, "refs/heads/main");
    seed.remote("origin", origin_path.to_str().unwrap())
        .unwrap();
    let worktree = clone_repo(&origin_path, &worktree_path);
    configure_user(&worktree);
    set_upstream(&worktree, "main", "origin/main");

    write_file(&seed_path, "README.md", "remote update\n");
    let remote_head = commit_all(&seed, "remote update");
    push_branch(&seed, "origin", "main");
    let tracked_before = worktree
        .find_reference("refs/remotes/origin/main")
        .unwrap()
        .target()
        .unwrap();

    for args in [
        vec!["fetch", "origin", "--dry-run", "--format", "json"],
        vec!["pull", "origin", "main", "--dry-run", "--format", "json"],
    ] {
        let output = run_gitrex(&worktree_path, &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response = json_response(&output);
        assert_eq!(response["ok"], true);
        assert_eq!(response["data"]["planning_source"], "gitrex_analysis");
        assert_eq!(response["data"]["network_access_required"], true);
        assert_eq!(response["data"]["network_access_during_planning"], false);
    }
    assert_eq!(
        worktree.head().unwrap().target().as_deref(),
        Some(initial.as_str())
    );
    assert_eq!(
        worktree
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .target()
            .as_deref(),
        Some(tracked_before.as_str())
    );
    assert_eq!(
        origin
            .find_reference("refs/heads/main")
            .unwrap()
            .target()
            .as_deref(),
        Some(remote_head.as_str())
    );

    let fetched = run_gitrex(&worktree_path, &["fetch", "origin", "--format", "json"]);
    assert!(
        fetched.status.success(),
        "{}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    let fetched = json_response(&fetched);
    assert_eq!(fetched["data"]["verification"]["status"], "verified");
    assert_eq!(fetched["data"]["state_changed"], true);
    assert!(fetched["data"]["confirmed_local_effects"]
        .as_array()
        .is_some_and(|effects| !effects.is_empty()));

    let pulled = run_gitrex(
        &worktree_path,
        &["pull", "origin", "main", "--format", "json"],
    );
    assert!(
        pulled.status.success(),
        "{}",
        String::from_utf8_lossy(&pulled.stderr)
    );
    let pulled = json_response(&pulled);
    assert_eq!(pulled["data"]["verification"]["status"], "verified");
    assert_eq!(pulled["data"]["after"]["head"], remote_head);

    write_file(&worktree_path, "local.txt", "local change\n");
    let local_head = commit_all(&worktree, "local update");
    let push = run_gitrex(&worktree_path, &["push", "--dry-run", "--format", "json"]);
    assert!(
        push.status.success(),
        "{}",
        String::from_utf8_lossy(&push.stderr)
    );
    let push = json_response(&push);
    assert_eq!(push["data"]["operation"], "push");
    assert_eq!(push["data"]["network_access_required"], true);
    assert_eq!(
        push["data"]["expected_remote_effects"][0]["commit_id"],
        local_head
    );
    assert_eq!(
        origin
            .find_reference("refs/heads/main")
            .unwrap()
            .target()
            .as_deref(),
        Some(remote_head.as_str())
    );
    assert_eq!(
        worktree.head().unwrap().target().as_deref(),
        Some(local_head.as_str())
    );

    let pushed = run_gitrex(&worktree_path, &["push", "--format", "json"]);
    assert!(
        pushed.status.success(),
        "{}",
        String::from_utf8_lossy(&pushed.stderr)
    );
    let pushed = json_response(&pushed);
    assert_eq!(pushed["data"]["verification"]["status"], "verified");
    assert_eq!(pushed["data"]["state_changed"], true);
    assert_eq!(
        pushed["data"]["confirmed_remote_effects"][0]["after_commit_id"],
        local_head
    );
    assert_eq!(
        origin
            .find_reference("refs/heads/main")
            .unwrap()
            .target()
            .as_deref(),
        Some(local_head.as_str())
    );
}

#[test]
fn precondition_change_is_structured_and_success_receipt_is_verified() {
    let temp = TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), "README.md", "gitrex\n");
    let head = commit_all(&repo, "initial commit");

    let stale = run_gitrex(
        temp.path(),
        &[
            "create-branch",
            "must-not-exist",
            "--expect-head",
            "0000000000000000000000000000000000000000",
            "--format",
            "json",
        ],
    );
    assert!(!stale.status.success());
    let stale = json_response(&stale);
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "PRECONDITION_CHANGED");
    assert_eq!(stale["error"]["details"]["observed_head"], head);
    assert_eq!(stale["error"]["details"]["observed_branch"], "main");
    assert!(repo.find_branch("must-not-exist").is_err());

    for expected in [
        vec!["--expect-branch", "other"],
        vec!["--expect-upstream", "origin/main"],
    ] {
        let mut args = vec!["create-branch", "also-must-not-exist"];
        args.extend(expected);
        args.extend(["--format", "json"]);
        let output = run_gitrex(temp.path(), &args);
        assert!(!output.status.success());
        let response = json_response(&output);
        assert_eq!(response["error"]["code"], "PRECONDITION_CHANGED");
        assert_eq!(response["error"]["details"]["observed_head"], head);
        assert!(repo.find_branch("also-must-not-exist").is_err());
    }

    let created = run_gitrex(
        temp.path(),
        &[
            "create-branch",
            "verified-branch",
            "--from",
            "main",
            "--format",
            "json",
        ],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let created = json_response(&created);
    assert_eq!(created["data"]["operation"], "create_branch");
    assert_eq!(created["data"]["state_changed"], true);
    assert_eq!(created["data"]["verification"]["status"], "verified");
    assert_eq!(created["data"]["before"]["head"], head);
    assert_eq!(created["data"]["after"]["branch"], "verified-branch");
    assert!(created["data"]["confirmed_local_effects"]
        .as_array()
        .is_some_and(|effects| !effects.is_empty()));
}

#[test]
fn shared_mutation_service_verifies_local_and_remote_effects() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("worktree");
    let repo = init_repo(&repo_path, "main");
    configure_user(&repo);
    write_file(&repo_path, "README.md", "base\n");
    let base = commit_all(&repo, "base");

    create_branch(&repo, "source", &base);
    support::checkout_branch(&repo, "source");
    write_file(&repo_path, "source.txt", "picked change\n");
    let source = commit_all(&repo, "source change");
    support::checkout_branch(&repo, "main");

    let client = GitClient::from_path(&repo_path);
    let detached = execute_verified(
        &client,
        MutationRequest::CheckoutDetached {
            target: source.clone(),
        },
    );
    assert_eq!(
        detached.receipt.after.head.as_deref(),
        Some(source.as_str())
    );
    assert_eq!(detached.receipt.after.branch, None);

    execute_verified(
        &client,
        MutationRequest::Switch {
            target: "main".into(),
        },
    );
    execute_verified(
        &client,
        MutationRequest::CreateBranch {
            name: "destination".into(),
            from: Some("main".into()),
        },
    );
    let picked = execute_verified(
        &client,
        MutationRequest::CherryPick {
            source: source.clone(),
            destination: "destination".into(),
        },
    );
    assert!(picked
        .receipt
        .confirmed_local_effects
        .iter()
        .any(|effect| effect.action == "cherry_pick_commit"));

    execute_verified(
        &client,
        MutationRequest::Reset {
            target: base.clone(),
            mode: OperationResetMode::Soft,
        },
    );
    execute_verified(
        &client,
        MutationRequest::Reset {
            target: base.clone(),
            mode: OperationResetMode::Mixed,
        },
    );
    fs::remove_file(repo_path.join("source.txt")).unwrap();
    execute_verified(
        &client,
        MutationRequest::Reset {
            target: source.clone(),
            mode: OperationResetMode::Hard,
        },
    );
    execute_verified(
        &client,
        MutationRequest::Checkout {
            target: "source".into(),
        },
    );
    execute_verified(
        &client,
        MutationRequest::Switch {
            target: "main".into(),
        },
    );

    execute_verified(
        &client,
        MutationRequest::CreateBranch {
            name: "merged".into(),
            from: Some("main".into()),
        },
    );
    execute_verified(
        &client,
        MutationRequest::Switch {
            target: "main".into(),
        },
    );
    let deleted = execute_verified(
        &client,
        MutationRequest::DeleteLocalBranch {
            branch: "merged".into(),
        },
    );
    assert!(deleted
        .receipt
        .confirmed_local_effects
        .iter()
        .any(|effect| effect.target == "refs/heads/merged"));

    create_branch(&repo, "cleanup", &base);
    let cleanup = execute_verified(
        &client,
        MutationRequest::Cleanup {
            base: "main".into(),
            branches: Some(vec!["cleanup".into()]),
            exclusions: Vec::new(),
            remotes: Vec::new(),
        },
    );
    assert!(cleanup
        .receipt
        .confirmed_local_effects
        .iter()
        .any(|effect| effect.target == "refs/heads/cleanup"));

    let origin_path = temp.path().join("origin.git");
    let origin = clone_bare_repo(&repo_path, &origin_path);
    set_remote_head(&origin, "refs/heads/source");
    repo.remote("origin", origin_path.to_str().unwrap())
        .unwrap();
    push_branch(&repo, "origin", "main");
    execute_verified(&client, MutationRequest::Fetch { remote: None });
    let remote_deleted = execute_verified(
        &client,
        MutationRequest::DeleteRemoteBranch {
            remote: "origin".into(),
            branch: "main".into(),
        },
    );
    assert!(remote_deleted
        .receipt
        .confirmed_remote_effects
        .iter()
        .any(|effect| effect.target == "origin/refs/heads/main"));
}

fn execute_verified(client: &GitClient, request: MutationRequest) -> OperationExecution {
    let execution = execute_mutation(client, &request, &OperationPreconditions::default())
        .unwrap_or_else(|error| panic!("{} failed before execution: {error}", request.name()));
    assert!(
        execution.failure.is_none(),
        "{} failed: {}",
        request.name(),
        execution
            .failure
            .as_ref()
            .map(|failure| failure.error.to_string())
            .unwrap_or_default()
    );
    assert_eq!(
        execution.receipt.verification.status,
        VerificationStatus::Verified,
        "{} verification failed: {:?}",
        request.name(),
        execution.receipt.verification
    );
    execution
}

fn run_gitrex(directory: &Path, args: &[&str]) -> Output {
    AssertCommand::cargo_bin("gitrex")
        .unwrap()
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn json_response(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "could not parse GitRex JSON output: {error}; stdout was {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
