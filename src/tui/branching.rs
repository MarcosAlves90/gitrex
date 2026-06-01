use crate::{
    domain::{BranchInfo, BranchKind, RepoStatus},
    tui::app::BranchPanel,
};

pub fn branch_filter_matches(branch_filter: &str, branch: &BranchInfo) -> bool {
    let query = branch_filter.trim();
    if query.is_empty() {
        return true;
    }

    let query = query.to_ascii_lowercase();
    let display_name = branch.display_name().to_ascii_lowercase();
    [
        branch.name.to_ascii_lowercase(),
        branch.commit.to_ascii_lowercase(),
        branch.subject.to_ascii_lowercase(),
        branch
            .upstream
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        display_name,
    ]
    .iter()
    .any(|haystack| haystack.contains(&query))
}

pub fn filtered_remote_branches<'a>(
    branches: &'a [BranchInfo],
    branch_filter: &str,
) -> Vec<&'a BranchInfo> {
    let mut branches = branches
        .iter()
        .filter(|branch| matches!(branch.kind, BranchKind::Remote))
        .filter(|branch| branch_filter_matches(branch_filter, branch))
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| {
        left.remote_name()
            .cmp(&right.remote_name())
            .then_with(|| left.branch_short_name().cmp(right.branch_short_name()))
            .then_with(|| right.commit.cmp(&left.commit))
    });
    branches
}

pub fn filtered_local_branches<'a>(
    branches: &'a [BranchInfo],
    branch_filter: &str,
) -> Vec<&'a BranchInfo> {
    let mut branches = branches
        .iter()
        .filter(|branch| matches!(branch.kind, BranchKind::Local))
        .filter(|branch| branch_filter_matches(branch_filter, branch))
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| {
        right
            .current
            .cmp(&left.current)
            .then_with(|| left.name.cmp(&right.name))
    });
    branches
}

pub fn ensure_selected_local_branch_visible(
    branches: &[BranchInfo],
    branch_filter: &str,
    selected_branch: &mut Option<String>,
) -> bool {
    let local_branches = filtered_local_branches(branches, branch_filter);
    if local_branches.is_empty() {
        let changed = selected_branch.is_some();
        *selected_branch = None;
        return changed;
    }

    let selected_visible = selected_branch.as_deref().and_then(|selected| {
        local_branches
            .iter()
            .position(|branch| branch.name == selected)
    });
    if selected_visible.is_some() {
        return false;
    }

    let selected = local_branches[0].name.clone();
    let changed = selected_branch.as_deref() != Some(selected.as_str());
    *selected_branch = Some(selected);
    changed
}

pub fn ensure_selected_remote_branch_visible(
    branches: &[BranchInfo],
    branch_filter: &str,
    selected_remote_branch: &mut Option<String>,
) -> bool {
    let remote_branches = filtered_remote_branches(branches, branch_filter);
    if remote_branches.is_empty() {
        let changed = selected_remote_branch.is_some();
        *selected_remote_branch = None;
        return changed;
    }

    let selected_visible = selected_remote_branch.as_deref().and_then(|selected| {
        remote_branches
            .iter()
            .position(|branch| branch.full_ref() == selected)
    });
    if selected_visible.is_some() {
        return false;
    }

    let selected = remote_branches[0].full_ref();
    let changed = selected_remote_branch.as_deref() != Some(selected.as_str());
    *selected_remote_branch = Some(selected);
    changed
}

pub fn selected_graph_ref(
    branch_panel: BranchPanel,
    selected_branch: Option<&BranchInfo>,
    selected_remote_branch: Option<&BranchInfo>,
    status_branch_name: Option<&str>,
) -> Option<String> {
    match branch_panel {
        BranchPanel::Local => selected_branch
            .map(|branch| branch.name.clone())
            .or_else(|| status_branch_name.map(|name| name.to_string())),
        BranchPanel::Remote => selected_remote_branch
            .map(|branch| branch.full_ref())
            .or_else(|| {
                selected_branch
                    .map(|branch| branch.name.clone())
                    .or_else(|| status_branch_name.map(|name| name.to_string()))
            }),
    }
}

pub fn selected_graph_label(
    branch_panel: BranchPanel,
    selected_branch: Option<&BranchInfo>,
    selected_remote_branch: Option<&BranchInfo>,
    status_branch_name: Option<&str>,
) -> Option<String> {
    match branch_panel {
        BranchPanel::Local => selected_branch
            .map(|branch| branch.name.clone())
            .or_else(|| status_branch_name.map(|name| name.to_string())),
        BranchPanel::Remote => selected_remote_branch
            .map(|branch| branch.display_name())
            .or_else(|| {
                selected_branch
                    .map(|branch| branch.name.clone())
                    .or_else(|| status_branch_name.map(|name| name.to_string()))
            }),
    }
}

pub fn current_sync_target(status: Option<&RepoStatus>) -> Option<(String, String)> {
    let status = status?;
    let upstream = status.upstream.as_deref()?;
    let (remote, _) = upstream.split_once('/')?;
    Some((remote.to_string(), status.branch_name.clone()))
}
