use std::sync::mpsc::Receiver;

use crate::{
    git::GitClient,
    tui::{
        app::{App, MessageKind},
        operations::{GitOperationRunner, OperationOutcome, OperationRequest},
    },
};

pub fn begin_operation(
    app: &mut App,
    runner: &GitOperationRunner,
    operation_rx: &mut Option<Receiver<OperationOutcome>>,
    operation: OperationRequest,
) {
    if operation_rx.is_some() {
        return;
    }

    let label = operation.loading_label();
    app.start_loading(label.clone());
    *operation_rx = Some(runner.spawn(operation));
    app.set_feedback(format!("{label}..."), MessageKind::Info);
}

pub fn refresh_selected_branch_history(
    app: &App,
    client: &GitClient,
) -> anyhow::Result<Option<crate::domain::BranchHistory>> {
    let Some(reference) = app
        .selected_graph_ref()
        .or_else(|| app.status.as_ref().map(|status| status.branch_name.clone()))
    else {
        return Ok(None);
    };

    client
        .history_for_ref(&reference)
        .map(Some)
        .map_err(anyhow::Error::msg)
}

pub fn finish_operation(app: &mut App, outcome: OperationOutcome) -> anyhow::Result<()> {
    app.stop_loading();
    match outcome {
        OperationOutcome::Success { snapshot, message } => {
            app.apply_snapshot(snapshot);
            app.set_feedback(message, MessageKind::Success);
        }
        OperationOutcome::SuccessWithRefreshWarning { message, warning } => {
            app.set_feedback(
                format!("{message}. {warning}. Press r to refresh."),
                MessageKind::Warning,
            );
        }
        OperationOutcome::Error(message) => {
            app.set_feedback(message, MessageKind::Error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{begin_operation, finish_operation, refresh_selected_branch_history};
    use crate::{
        domain::{BranchHistory, RepoSnapshot, RepoStatus},
        git::GitClient,
        tui::{
            app::{App, MessageKind},
            operations::{GitOperationRunner, OperationOutcome, OperationRequest},
        },
    };

    #[test]
    fn operation_flow_handles_begin_reentry_success_and_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let client = GitClient::from_path(temp.path());
        let runner = GitOperationRunner::new(<GitClient as Clone>::clone(&client));
        let mut app = App::new();
        let mut rx = None;

        begin_operation(
            &mut app,
            &runner,
            &mut rx,
            OperationRequest::Checkout {
                branch: "missing".to_string(),
            },
        );
        assert!(rx.is_some());
        assert!(app.footer_text().contains("Checking out missing"));

        begin_operation(
            &mut app,
            &runner,
            &mut rx,
            OperationRequest::Switch {
                branch: "ignored".to_string(),
            },
        );
        assert!(rx.is_some());

        finish_operation(
            &mut app,
            OperationOutcome::Error("failed safely".to_string()),
        )
        .unwrap();
        assert_eq!(app.message_kind, MessageKind::Error);
        assert_eq!(app.footer_text(), "failed safely");

        let snapshot = RepoSnapshot {
            status: RepoStatus {
                branch_name: "main".to_string(),
                upstream: None,
                ahead: 0,
                behind: 0,
                files: Vec::new(),
            },
            branches: Vec::new(),
            history: BranchHistory::from_graph(Vec::new()),
            selected_branch: None,
        };
        finish_operation(
            &mut app,
            OperationOutcome::Success {
                snapshot,
                message: "done".to_string(),
            },
        )
        .unwrap();
        assert_eq!(app.message_kind, MessageKind::Success);
        assert_eq!(app.footer_text(), "done");

        finish_operation(
            &mut app,
            OperationOutcome::SuccessWithRefreshWarning {
                message: "Push complete".to_string(),
                warning: "Repository view refresh failed: unavailable".to_string(),
            },
        )
        .unwrap();
        assert_eq!(app.message_kind, MessageKind::Warning);
        assert!(app.footer_text().contains("Push complete"));
        assert!(app.footer_text().contains("Press r to refresh"));
    }

    #[test]
    fn refresh_history_returns_none_without_context_and_reads_selected_ref() {
        let empty = tempfile::TempDir::new().unwrap();
        let app = App::new();
        let client = GitClient::from_path(empty.path());
        assert!(refresh_selected_branch_history(&app, &client)
            .unwrap()
            .is_none());

        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo = crate::test_support::init_repo(repo_dir.path(), "main");
        crate::test_support::configure_user(&repo);
        crate::test_support::write_file(repo_dir.path(), "README.md", "base\n");
        crate::test_support::commit_all(&repo, "base");

        let mut app = App::new();
        app.status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: None,
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });
        let client = GitClient::from_path(repo_dir.path());
        let history = refresh_selected_branch_history(&app, &client)
            .unwrap()
            .unwrap();
        assert_eq!(history.commits.len(), 1);
        assert_eq!(history.commits[0].subject, "base");
    }
}
