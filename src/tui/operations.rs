use std::thread;
use std::{
    collections::BTreeSet,
    sync::mpsc::{self, Receiver},
};

use crate::{
    app::operations::execute_mutation,
    domain::{
        branch::{BranchCleanupOutcome, BranchCleanupState},
        MutationRequest, OperationPreconditions, OperationResetMode, RepoSnapshot, Result,
    },
    git::{CommitComparison, GitClient, ResetMode},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
    CompareCommits {
        left_reference: String,
        right_reference: String,
    },
    CherryPick {
        source: String,
        destination: String,
    },
    Reset {
        target: String,
        mode: ResetMode,
        expected_branch: String,
        expected_head: String,
    },
    Checkout {
        branch: String,
    },
    CheckoutDetached {
        target: String,
    },
    Switch {
        branch: String,
    },
    CreateBranch {
        branch: String,
        start_point: String,
    },
    DeleteLocalBranch {
        branch: String,
    },
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    CleanupLocalBranches {
        branches: Vec<String>,
        base: String,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        remote: Option<String>,
        branch: Option<String>,
    },
}

impl OperationRequest {
    pub fn loading_label(&self) -> String {
        match self {
            OperationRequest::CompareCommits {
                left_reference,
                right_reference,
            } => format!("Comparing {left_reference} with {right_reference}"),
            OperationRequest::CherryPick {
                source,
                destination,
            } => format!("Cherry-picking {source} onto {destination}"),
            OperationRequest::Reset {
                target,
                mode,
                expected_branch,
                ..
            } => format!("Resetting {expected_branch} to {target} ({})", mode.label()),
            OperationRequest::Checkout { branch } => format!("Checking out {branch}"),
            OperationRequest::CheckoutDetached { target } => {
                format!("Checking out detached HEAD at {target}")
            }
            OperationRequest::Switch { branch } => format!("Switching to {branch}"),
            OperationRequest::CreateBranch {
                branch,
                start_point,
            } => {
                format!("Creating {branch} from {start_point}")
            }
            OperationRequest::DeleteLocalBranch { branch } => {
                format!("Deleting local branch {branch}")
            }
            OperationRequest::DeleteRemoteBranch { remote, branch } => {
                format!("Deleting remote branch {remote}/{branch}")
            }
            OperationRequest::CleanupLocalBranches { branches, .. } => {
                format!("Cleaning up {} local branches", branches.len())
            }
            OperationRequest::Pull { remote, branch }
            | OperationRequest::Push { remote, branch } => {
                match (remote.as_deref(), branch.as_deref()) {
                    (Some(remote), Some(branch)) => format!("{remote}/{branch}"),
                    _ => String::from("current branch"),
                }
            }
        }
    }

    pub fn success_label(&self) -> String {
        match self {
            OperationRequest::CompareCommits { .. } => String::from("Comparison complete"),
            OperationRequest::CherryPick {
                source,
                destination,
            } => format!("Cherry-picked {source} onto {destination}"),
            OperationRequest::Reset {
                target,
                mode,
                expected_branch,
                ..
            } => format!("Reset {expected_branch} to {target} ({})", mode.label()),
            OperationRequest::Checkout { branch } => format!("Checked out {branch}"),
            OperationRequest::CheckoutDetached { target } => {
                format!("Checked out detached HEAD at {target}")
            }
            OperationRequest::Switch { branch } => format!("Switched to {branch}"),
            OperationRequest::CreateBranch {
                branch,
                start_point,
            } => {
                format!("Created {branch} from {start_point}")
            }
            OperationRequest::DeleteLocalBranch { branch } => {
                format!("Deleted local branch {branch}")
            }
            OperationRequest::DeleteRemoteBranch { remote, branch } => {
                format!("Deleted remote branch {remote}/{branch}")
            }
            OperationRequest::CleanupLocalBranches { branches, .. } => {
                format!("Cleaned up {} local branches", branches.len())
            }
            OperationRequest::Pull { remote, branch } => {
                match (remote.as_deref(), branch.as_deref()) {
                    (Some(remote), Some(branch)) => format!("Pulled {remote}/{branch}"),
                    _ => String::from("Pull complete."),
                }
            }
            OperationRequest::Push { remote, branch } => {
                match (remote.as_deref(), branch.as_deref()) {
                    (Some(remote), Some(branch)) => format!("Pushed {remote}/{branch}"),
                    _ => String::from("Push complete."),
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum OperationOutcome {
    Comparison(CommitComparison),
    Success {
        snapshot: RepoSnapshot,
        message: String,
    },
    SuccessWithRefreshWarning {
        message: String,
        warning: String,
    },
    Error(String),
    StateChangedFailure {
        snapshot: Option<RepoSnapshot>,
        message: String,
        refresh_warning: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub deleted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub details: String,
}

const CLEANUP_REPORT_MARKER: &str = "GITREX_CLEANUP_REPORT_V1";

pub fn parse_cleanup_report(message: &str) -> Option<CleanupReport> {
    let mut lines = message.lines();
    if lines.next()? != CLEANUP_REPORT_MARKER {
        return None;
    }
    let mut counts = lines.next()?.split('\t');
    let deleted = counts.next()?.parse().ok()?;
    let skipped = counts.next()?.parse().ok()?;
    let failed = counts.next()?.parse().ok()?;
    if counts.next().is_some() {
        return None;
    }
    Some(CleanupReport {
        deleted,
        skipped,
        failed,
        details: lines.collect::<Vec<_>>().join("\n"),
    })
}

#[derive(Debug, Clone, Default)]
pub struct GitOperationRunner {
    client: GitClient,
}

impl GitOperationRunner {
    pub fn new(client: GitClient) -> Self {
        Self { client }
    }

    pub fn spawn(&self, request: OperationRequest) -> Receiver<OperationOutcome> {
        let client = <GitClient as Clone>::clone(&self.client);
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let outcome = execute_operation(client, request);
            let _ = tx.send(outcome);
        });

        rx
    }
}

fn execute_operation(client: GitClient, request: OperationRequest) -> OperationOutcome {
    let success_message = request.success_label();
    let result = match request {
        OperationRequest::CompareCommits {
            left_reference,
            right_reference,
        } => {
            return match client.compare_commits(&left_reference, &right_reference) {
                Ok(comparison) => OperationOutcome::Comparison(comparison),
                Err(error) => OperationOutcome::Error(error.to_string()),
            };
        }
        OperationRequest::CherryPick {
            source,
            destination,
        } => return execute_cherry_pick(client, source, destination),
        OperationRequest::Reset {
            target,
            mode,
            expected_branch,
            expected_head,
        } => {
            return execute_reset(client, target, mode, expected_branch, expected_head);
        }
        OperationRequest::CleanupLocalBranches { branches, base } => {
            return execute_cleanup(client, branches, base);
        }
        OperationRequest::Checkout { branch } => execute_shared_mutation(
            &client,
            MutationRequest::Checkout {
                target: branch.clone(),
            },
            format!("Checked out {branch}"),
        ),
        OperationRequest::CheckoutDetached { target } => execute_shared_mutation(
            &client,
            MutationRequest::CheckoutDetached {
                target: target.clone(),
            },
            format!("Checked out detached HEAD at {target}"),
        ),
        OperationRequest::Switch { branch } => execute_shared_mutation(
            &client,
            MutationRequest::Switch {
                target: branch.clone(),
            },
            format!("Switched to {branch}"),
        ),
        OperationRequest::CreateBranch {
            branch,
            start_point,
        } => execute_shared_mutation(
            &client,
            MutationRequest::CreateBranch {
                name: branch.clone(),
                from: Some(start_point.clone()),
            },
            format!("Created {branch} from {start_point}"),
        ),
        OperationRequest::DeleteLocalBranch { branch } => execute_shared_mutation(
            &client,
            MutationRequest::DeleteLocalBranch {
                branch: branch.clone(),
            },
            format!("Deleted local branch {branch}"),
        ),
        OperationRequest::DeleteRemoteBranch { remote, branch } => execute_shared_mutation(
            &client,
            MutationRequest::DeleteRemoteBranch {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            format!("Deleted remote branch {remote}/{branch}"),
        ),
        OperationRequest::Pull { remote, branch } => execute_shared_mutation(
            &client,
            MutationRequest::Pull {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            String::from("Pull complete."),
        ),
        OperationRequest::Push { remote, branch } => execute_shared_mutation(
            &client,
            MutationRequest::Push {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            String::from("Push complete."),
        ),
    };

    match result {
        Ok(_) => match client.snapshot() {
            Ok(snapshot) => OperationOutcome::Success {
                snapshot,
                message: success_message,
            },
            Err(error) => OperationOutcome::SuccessWithRefreshWarning {
                message: success_message,
                warning: format!("Repository view refresh failed: {error}"),
            },
        },
        Err(error) => OperationOutcome::Error(error.to_string()),
    }
}

fn execute_shared_mutation(
    client: &GitClient,
    request: MutationRequest,
    message: String,
) -> Result<String> {
    let execution = execute_mutation(client, &request, &OperationPreconditions::default())?;
    if let Some(failure) = execution.failure {
        return Err(failure.error);
    }
    Ok(message)
}

fn execute_cherry_pick(client: GitClient, source: String, destination: String) -> OperationOutcome {
    let request = MutationRequest::CherryPick {
        source: source.clone(),
        destination: destination.clone(),
    };
    let execution = match execute_mutation(&client, &request, &OperationPreconditions::default()) {
        Ok(execution) => execution,
        Err(error) => return OperationOutcome::Error(error.to_string()),
    };
    let success_message = format!(
        "Cherry-picked {} onto local branch {destination}",
        short_oid(&source)
    );
    let failure_message = execution.failure.as_ref().map(|failure| {
        let detail = failure.error.to_string();
        if let Some((_, detail)) = detail.split_once("cherry-pick conflict:") {
            format!(
                "Cherry-pick conflict on local branch {destination} for {}. Resolve conflicts, then run `git cherry-pick --continue` or `git cherry-pick --abort`.{}",
                short_oid(&source),
                detail
            )
        } else if let Some((_, detail)) = detail.split_once("cherry-pick stopped:") {
            format!(
                "Cherry-pick is paused on local branch {destination} without unresolved file conflicts for {}. If the change is already applied or empty, run `git cherry-pick --skip`; run `git cherry-pick --abort` to cancel.{}",
                short_oid(&source),
                detail
            )
        } else {
            format!(
                "Cherry-pick attempt failed on local branch {destination} for {}. Inspect the repository state before continuing. {detail}",
                short_oid(&source)
            )
        }
    });
    finish_shared_execution(&client, execution, success_message, failure_message)
}

fn execute_reset(
    client: GitClient,
    target: String,
    mode: ResetMode,
    expected_branch: String,
    expected_head: String,
) -> OperationOutcome {
    let operation_mode = match mode {
        ResetMode::Soft => OperationResetMode::Soft,
        ResetMode::Mixed => OperationResetMode::Mixed,
        ResetMode::Hard => OperationResetMode::Hard,
    };
    let request = MutationRequest::Reset {
        target: target.clone(),
        mode: operation_mode,
    };
    let preconditions = OperationPreconditions {
        expect_head: Some(expected_head),
        expect_branch: Some(expected_branch.clone()),
        expect_upstream: None,
    };
    let execution = match execute_mutation(&client, &request, &preconditions) {
        Ok(execution) => execution,
        Err(error) => return OperationOutcome::Error(error.to_string()),
    };
    let success_message = format!(
        "Reset local branch {expected_branch} ({}) from {} to {}",
        mode.label(),
        execution
            .receipt
            .before
            .head
            .as_deref()
            .map(short_oid)
            .unwrap_or_default(),
        execution
            .receipt
            .after
            .head
            .as_deref()
            .map(short_oid)
            .unwrap_or_else(|| short_oid(&target))
    );
    let failure_message = execution.failure.as_ref().map(|failure| {
        format!(
            "Reset attempt failed for local branch {expected_branch} ({}) at target {}. {}",
            mode.label(),
            short_oid(&target),
            failure.error
        )
    });
    finish_shared_execution(&client, execution, success_message, failure_message)
}

fn finish_shared_execution(
    client: &GitClient,
    execution: crate::domain::OperationExecution,
    success_message: String,
    failure_message: Option<String>,
) -> OperationOutcome {
    if let Some(message) = failure_message {
        return if execution.receipt.state_changed {
            state_changed_failure(client, message)
        } else {
            OperationOutcome::Error(message)
        };
    }
    match client.snapshot() {
        Ok(snapshot) => OperationOutcome::Success {
            snapshot,
            message: success_message,
        },
        Err(error) => OperationOutcome::SuccessWithRefreshWarning {
            message: success_message,
            warning: format!("Repository view refresh failed: {error}"),
        },
    }
}

fn state_changed_failure(client: &GitClient, message: String) -> OperationOutcome {
    match client.snapshot() {
        Ok(snapshot) => OperationOutcome::StateChangedFailure {
            snapshot: Some(snapshot),
            message,
            refresh_warning: None,
        },
        Err(error) => OperationOutcome::StateChangedFailure {
            snapshot: None,
            message,
            refresh_warning: Some(format!("Repository view refresh failed: {error}")),
        },
    }
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(8).collect()
}

fn execute_cleanup(client: GitClient, branches: Vec<String>, base: String) -> OperationOutcome {
    let request = MutationRequest::Cleanup {
        base: base.clone(),
        branches: Some(branches.clone()),
        exclusions: Vec::new(),
        remotes: Vec::new(),
    };
    let outcomes: Vec<BranchCleanupOutcome> =
        match execute_mutation(&client, &request, &OperationPreconditions::default()) {
            Err(error) => branches
                .into_iter()
                .map(|branch| BranchCleanupOutcome {
                    branch,
                    state: BranchCleanupState::Failed,
                    detail: Some(error.to_string()),
                })
                .collect(),
            Ok(execution) => {
                let planned = execution
                    .plan
                    .expected_local_effects
                    .iter()
                    .filter_map(|effect| effect.target.strip_prefix("refs/heads/"))
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>();
                let deleted = execution
                    .receipt
                    .confirmed_local_effects
                    .iter()
                    .filter_map(|effect| effect.target.strip_prefix("refs/heads/"))
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>();
                let failure = execution
                    .failure
                    .as_ref()
                    .map(|failure| failure.error.to_string());
                branches
                    .into_iter()
                    .map(|branch| {
                        let (state, detail) = if deleted.contains(&branch) {
                            (BranchCleanupState::Deleted, None)
                        } else if !planned.contains(&branch) {
                            (
                                BranchCleanupState::Skipped,
                                Some(String::from("branch was no longer eligible for cleanup")),
                            )
                        } else {
                            (
                                BranchCleanupState::Failed,
                                Some(failure.clone().unwrap_or_else(|| {
                                    String::from("planned branch was not deleted")
                                })),
                            )
                        };
                        BranchCleanupOutcome {
                            branch,
                            state,
                            detail,
                        }
                    })
                    .collect()
            }
        };
    let message = format_cleanup_report(&outcomes);

    match client.snapshot() {
        Ok(snapshot) => OperationOutcome::Success { snapshot, message },
        Err(error) => OperationOutcome::SuccessWithRefreshWarning {
            message,
            warning: format!("Repository view refresh failed: {error}"),
        },
    }
}

fn format_cleanup_report(outcomes: &[BranchCleanupOutcome]) -> String {
    let deleted = outcomes
        .iter()
        .filter(|outcome| outcome.state == BranchCleanupState::Deleted)
        .count();
    let skipped = outcomes
        .iter()
        .filter(|outcome| outcome.state == BranchCleanupState::Skipped)
        .count();
    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.state == BranchCleanupState::Failed)
        .count();
    let mut message = format!("{CLEANUP_REPORT_MARKER}\n{deleted}\t{skipped}\t{failed}");
    for outcome in outcomes {
        let label = match outcome.state {
            BranchCleanupState::Deleted => "deleted",
            BranchCleanupState::Skipped => "skipped",
            BranchCleanupState::Failed => "failed",
        };
        message.push('\n');
        message.push_str(label);
        message.push_str(": ");
        message.push_str(&outcome.branch);
        if let Some(detail) = outcome.detail.as_deref() {
            let detail = detail.replace(['\n', '\r', '\t'], " ");
            message.push_str(" (");
            message.push_str(&detail);
            message.push(')');
        }
    }
    message
}

#[cfg(test)]
mod tests {
    use super::{execute_operation, GitOperationRunner, OperationOutcome, OperationRequest};
    use crate::git::GitClient;
    use crate::test_support::{
        checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
        current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
        CurrentDirGuard,
    };

    #[test]
    fn request_labels_are_clear() {
        assert_eq!(
            OperationRequest::Pull {
                remote: Some("origin".into()),
                branch: Some("main".into())
            }
            .loading_label(),
            "origin/main"
        );
        assert_eq!(
            OperationRequest::Pull {
                remote: Some("origin".into()),
                branch: Some("main".into())
            }
            .success_label(),
            "Pulled origin/main"
        );
        assert_eq!(
            OperationRequest::CreateBranch {
                branch: "feature/login".into(),
                start_point: "main".into()
            }
            .loading_label(),
            "Creating feature/login from main"
        );
        assert_eq!(
            OperationRequest::DeleteLocalBranch {
                branch: "feature/login".into()
            }
            .loading_label(),
            "Deleting local branch feature/login"
        );
        assert_eq!(
            OperationRequest::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "feature/login".into()
            }
            .success_label(),
            "Deleted remote branch origin/feature/login"
        );
        assert_eq!(
            OperationRequest::CheckoutDetached {
                target: "refs/remotes/origin/main".into()
            }
            .loading_label(),
            "Checking out detached HEAD at refs/remotes/origin/main"
        );
    }

    #[test]
    fn build_snapshot_includes_all_log_entries() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        for index in 0..15 {
            write_file(temp.path(), "README.md", &format!("commit {index}\n"));
            commit_all(&repo, &format!("commit {index}"));
        }
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        let snapshot = client.snapshot().unwrap();

        assert!(snapshot.history.commits.len() > 12);
    }

    #[test]
    fn build_snapshot_stays_on_current_branch_history() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base commit");
        create_branch(&repo, "feature/login", "HEAD");
        checkout_branch(&repo, "feature/login");
        write_file(temp.path(), "README.md", "feature work\n");
        commit_all(&repo, "feature work");
        checkout_branch(&repo, "main");
        write_file(temp.path(), "README.md", "main work\n");
        commit_all(&repo, "main work");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        let snapshot = client.snapshot().unwrap();

        assert!(snapshot
            .history
            .commits
            .iter()
            .any(|entry| entry.subject == "main work"));
        assert!(!snapshot
            .history
            .commits
            .iter()
            .any(|entry| entry.subject == "feature work"));
    }

    #[test]
    fn request_labels_cover_every_operation_variant() {
        let requests = [
            OperationRequest::Checkout {
                branch: "main".into(),
            },
            OperationRequest::CheckoutDetached {
                target: "abc123".into(),
            },
            OperationRequest::Switch {
                branch: "main".into(),
            },
            OperationRequest::CreateBranch {
                branch: "feature".into(),
                start_point: "main".into(),
            },
            OperationRequest::DeleteLocalBranch {
                branch: "feature".into(),
            },
            OperationRequest::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "feature".into(),
            },
            OperationRequest::Pull {
                remote: None,
                branch: None,
            },
            OperationRequest::Push {
                remote: None,
                branch: None,
            },
        ];

        for request in requests {
            assert!(!request.loading_label().is_empty());
            assert!(!request.success_label().is_empty());
        }

        assert_eq!(
            OperationRequest::Push {
                remote: Some("origin".into()),
                branch: Some("main".into()),
            }
            .loading_label(),
            "origin/main"
        );
        assert_eq!(
            OperationRequest::Push {
                remote: Some("origin".into()),
                branch: Some("main".into()),
            }
            .success_label(),
            "Pushed origin/main"
        );
    }

    #[test]
    fn execute_operation_covers_local_mutations_and_failures() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base");
        create_branch(&repo, "feature/login", "HEAD");

        let client = GitClient::from_path(temp.path());

        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::Checkout {
                    branch: "feature/login".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::Switch {
                    branch: "main".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::CreateBranch {
                    branch: "topic".to_string(),
                    start_point: "main".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::DeleteLocalBranch {
                    branch: "feature/login".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::CheckoutDetached {
                    target: "HEAD".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::Checkout {
                    branch: "definitely-missing".to_string(),
                },
            ),
            OperationOutcome::Error(_)
        ));

        let runner = GitOperationRunner::new(client);
        let outcome = runner
            .spawn(OperationRequest::Switch {
                branch: "definitely-missing".to_string(),
            })
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(outcome, OperationOutcome::Error(_)));
    }

    #[test]
    fn execute_operation_covers_pull_push_and_remote_delete() {
        let temp = tempfile::TempDir::new().unwrap();
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

        write_file(&collaborator, "README.md", "remote update\n");
        commit_all(&collaborator_repo, "remote update");
        push_branch(&collaborator_repo, "origin", "main");

        let client = GitClient::from_path(&worktree);
        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::Pull {
                    remote: Some("origin".to_string()),
                    branch: Some("main".to_string()),
                },
            ),
            OperationOutcome::Success { .. }
        ));

        write_file(&worktree, "feature.txt", "feature\n");
        commit_all(&worktree_repo, "feature work");
        create_branch(&worktree_repo, "feature/login", "HEAD");
        checkout_branch(&worktree_repo, "feature/login");

        assert!(matches!(
            execute_operation(
                <GitClient as Clone>::clone(&client),
                OperationRequest::Push {
                    remote: Some("origin".to_string()),
                    branch: Some("feature/login".to_string()),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(origin_repo
            .find_reference("refs/heads/feature/login")
            .is_ok());

        assert!(matches!(
            execute_operation(
                client,
                OperationRequest::DeleteRemoteBranch {
                    remote: "origin".to_string(),
                    branch: "feature/login".to_string(),
                },
            ),
            OperationOutcome::Success { .. }
        ));
        assert!(origin_repo
            .find_reference("refs/heads/feature/login")
            .is_err());
    }
}
