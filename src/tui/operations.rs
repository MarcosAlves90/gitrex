use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::{
    domain::{BranchInfo, CommitSummary, RepoStatus},
    git::GitClient,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
    Checkout { branch: String },
    Switch { branch: String },
    CreateBranch {
        branch: String,
        start_point: String,
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
            OperationRequest::Switch { branch } => format!("Switching to {branch}"),
            OperationRequest::CreateBranch { branch, start_point } => {
                format!("Creating {branch} from {start_point}")
            }
            OperationRequest::Pull { remote, branch } | OperationRequest::Push { remote, branch } => {
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
            OperationRequest::Switch { branch } => format!("Switched to {branch}"),
            OperationRequest::CreateBranch { branch, start_point } => {
                format!("Created {branch} from {start_point}")
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

#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub status: Option<RepoStatus>,
    pub branches: Vec<BranchInfo>,
    pub log: Vec<CommitSummary>,
    pub selected_branch: usize,
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

pub fn build_snapshot(client: &GitClient) -> Result<RepoSnapshot, String> {
    let status = client.status().map_err(|error| error.to_string())?;
    let branches = client.branches().map_err(|error| error.to_string())?;
    let log = client.log(12).map_err(|error| error.to_string())?;
    let selected_branch = branches
        .iter()
        .position(|branch| branch.current && matches!(branch.kind, crate::domain::BranchKind::Local))
        .or_else(|| {
            branches
                .iter()
                .position(|branch| matches!(branch.kind, crate::domain::BranchKind::Local) && branch.name == status.branch_name)
        })
        .unwrap_or(0);

    Ok(RepoSnapshot {
        status: Some(status),
        branches,
        log,
        selected_branch,
    })
}

fn execute_operation(client: GitClient, request: OperationRequest) -> OperationOutcome {
    let success_message = request.success_label();
    let result = match request {
        OperationRequest::Checkout { branch } => client
            .checkout(&branch)
            .map(|_| format!("Checked out {branch}")),
        OperationRequest::Switch { branch } => client
            .switch(&branch)
            .map(|_| format!("Switched to {branch}")),
        OperationRequest::CreateBranch { branch, start_point } => client
            .create_branch(&branch, Some(&start_point))
            .map(|_| format!("Created {branch} from {start_point}")),
        OperationRequest::Pull { remote, branch } => client
            .pull(remote.as_deref(), branch.as_deref())
            .map(|_| String::from("Pull complete.")),
        OperationRequest::Push { remote, branch } => client
            .push(remote.as_deref(), branch.as_deref())
            .map(|_| String::from("Push complete.")),
    };

    match result {
        Ok(_) => match build_snapshot(&client) {
            Ok(snapshot) => OperationOutcome::Success {
                snapshot,
                message: success_message,
            },
            Err(error) => OperationOutcome::Error(error),
        },
        Err(error) => OperationOutcome::Error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::OperationRequest;

    #[test]
    fn request_labels_are_clear() {
        assert_eq!(OperationRequest::Pull { remote: Some("origin".into()), branch: Some("main".into()) }.loading_label(), "origin/main");
        assert_eq!(OperationRequest::Pull { remote: Some("origin".into()), branch: Some("main".into()) }.success_label(), "Pulled origin/main");
        assert_eq!(OperationRequest::CreateBranch { branch: "feature/login".into(), start_point: "main".into() }.loading_label(), "Creating feature/login from main");
    }
}
