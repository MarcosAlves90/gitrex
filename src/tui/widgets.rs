use super::app::{BranchPanel, View};

pub fn mode_label(view: View) -> &'static str {
    match view {
        View::Status => "status",
        View::Branches => "branches",
        View::Log => "graph",
    }
}

pub fn actions_copy(
    selected_branch: Option<&str>,
    sync_target: Option<&str>,
    panel: BranchPanel,
) -> String {
    let branch = selected_branch.unwrap_or("no branch selected");
    let sync = sync_target.unwrap_or("no upstream");
    let branch_action = match panel {
        BranchPanel::Local => "Enter = open local branch actions",
        BranchPanel::Remote => "Enter = open remote branch actions",
    };
    vec![
        "Keys:".to_string(),
        "j/k or arrows = move".to_string(),
        "1/2/3 = change pane".to_string(),
        "Tab / Shift+Tab = switch local and remote branch panels".to_string(),
        format!("Active branch panel: {}", panel.label()),
        branch_action.to_string(),
        "/ = search branches".to_string(),
        "In local branch actions: checkout, switch, pull, push, or create branch from source"
            .to_string(),
        "In remote branch actions: create a local branch or checkout detached HEAD".to_string(),
        "r = refresh".to_string(),
        "q = quit".to_string(),
        String::new(),
        "Selected branch:".to_string(),
        branch.to_string(),
        "Sync target:".to_string(),
        sync.to_string(),
        "Branch view:".to_string(),
        "remote refs are grouped by remote name".to_string(),
        "local refs show synced vs local-only status".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{actions_copy, mode_label};
    use crate::tui::app::{BranchPanel, View};

    #[test]
    fn mode_label_matches_view_names() {
        assert_eq!(mode_label(View::Status), "status");
        assert_eq!(mode_label(View::Branches), "branches");
        assert_eq!(mode_label(View::Log), "graph");
    }

    #[test]
    fn actions_copy_mentions_selected_branch() {
        let copy = actions_copy(
            Some("feature/login"),
            Some("origin/feature/login"),
            BranchPanel::Local,
        );
        assert!(copy.contains("feature/login"));
        assert!(copy.contains("Enter = open local branch actions"));
        assert!(copy.contains("/ = search branches"));
        assert!(copy.contains("origin/feature/login"));
        assert!(copy.contains("create branch from source"));
    }
}
