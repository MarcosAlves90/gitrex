use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::{domain::RepoSnapshot, git::GitClient};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
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
    Success {
        snapshot: RepoSnapshot,
        message: String,
    },
    Error(String),
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
        OperationRequest::Checkout { branch } => client
            .checkout(&branch)
            .map(|_| format!("Checked out {branch}")),
        OperationRequest::CheckoutDetached { target } => client
            .checkout(&target)
            .map(|_| format!("Checked out detached HEAD at {target}")),
        OperationRequest::Switch { branch } => client
            .switch(&branch)
            .map(|_| format!("Switched to {branch}")),
        OperationRequest::CreateBranch {
            branch,
            start_point,
        } => client
            .create_branch(&branch, Some(&start_point))
            .map(|_| format!("Created {branch} from {start_point}")),
        OperationRequest::DeleteLocalBranch { branch } => client
            .delete_local_branch(&branch)
            .map(|_| format!("Deleted local branch {branch}")),
        OperationRequest::DeleteRemoteBranch { remote, branch } => client
            .delete_remote_branch(&remote, &branch)
            .map(|_| format!("Deleted remote branch {remote}/{branch}")),
        OperationRequest::Pull { remote, branch } => client
            .pull(remote.as_deref(), branch.as_deref())
            .map(|_| String::from("Pull complete.")),
        OperationRequest::Push { remote, branch } => client
            .push(remote.as_deref(), branch.as_deref())
            .map(|_| String::from("Push complete.")),
    };

    match result {
        Ok(_) => match client.snapshot() {
            Ok(snapshot) => OperationOutcome::Success {
                snapshot,
                message: success_message,
            },
            Err(error) => OperationOutcome::Error(error.to_string()),
        },
        Err(error) => OperationOutcome::Error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::OperationRequest;
    use crate::git::GitClient;
    use crate::test_support::{
        checkout_branch, commit_all, configure_user, create_branch, current_dir_lock, init_repo,
        write_file, CurrentDirGuard,
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
}
