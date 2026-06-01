use crate::domain::{BranchKind, RepoSnapshot, Result};

use super::GitClient;

pub fn read_snapshot(client: &GitClient) -> Result<RepoSnapshot> {
    client.refresh_remote_refs()?;
    let status = client.status()?;
    let branches = client.branches()?;
    let selected_branch = select_branch(&branches, status.branch_name.as_str());
    let graph_ref = selected_branch
        .as_deref()
        .unwrap_or(status.branch_name.as_str());
    let history = client.history_for_ref(graph_ref)?;

    Ok(RepoSnapshot {
        status,
        branches,
        history,
        selected_branch,
    })
}

fn select_branch(branches: &[crate::domain::BranchInfo], fallback: &str) -> Option<String> {
    branches
        .iter()
        .position(|branch| branch.current && matches!(branch.kind, BranchKind::Local))
        .or_else(|| {
            branches.iter().position(|branch| {
                matches!(branch.kind, BranchKind::Local) && branch.name == fallback
            })
        })
        .and_then(|index| branches.get(index).map(|branch| branch.name.clone()))
}
