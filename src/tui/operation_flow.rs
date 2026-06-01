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
    let Some(reference) = app.selected_graph_ref().or_else(|| {
        app.status
            .as_ref()
            .map(|status| status.branch_name.clone())
    }) else {
        return Ok(None);
    };

    client
        .history_for_ref(&reference)
        .map(Some)
        .map_err(anyhow::Error::msg)
}

pub fn finish_operation(
    app: &mut App,
    outcome: OperationOutcome,
) -> anyhow::Result<()> {
    app.stop_loading();
    match outcome {
        OperationOutcome::Success { snapshot, message } => {
            app.apply_snapshot(snapshot);
            app.set_feedback(message, MessageKind::Success);
        }
        OperationOutcome::Error(message) => {
            app.set_feedback(message, MessageKind::Error);
        }
    }
    Ok(())
}
