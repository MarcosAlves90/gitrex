use super::app::View;

pub fn mode_label(view: View) -> &'static str {
    match view {
        View::Status => "status",
        View::Branches => "branches",
        View::Log => "graph",
    }
}

pub fn actions_copy(selected_branch: Option<&str>, sync_target: Option<&str>) -> String {
    let branch = selected_branch.unwrap_or("no branch selected");
    let sync = sync_target.unwrap_or("no upstream");
    [
        "Keys:",
        "j/k or arrows = move",
        "1/2/3 = change pane",
        "Enter = open branch actions",
        "In branch actions: create branch from source",
        "r = refresh",
        "q = quit",
        "",
        "Selected branch:",
        branch,
        "Sync target:",
        sync,
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{actions_copy, mode_label};
    use crate::tui::app::View;

    #[test]
    fn mode_label_matches_view_names() {
        assert_eq!(mode_label(View::Status), "status");
        assert_eq!(mode_label(View::Branches), "branches");
        assert_eq!(mode_label(View::Log), "graph");
    }

    #[test]
    fn actions_copy_mentions_selected_branch() {
        let copy = actions_copy(Some("feature/login"), Some("origin/feature/login"));
        assert!(copy.contains("feature/login"));
        assert!(copy.contains("Enter = open branch actions"));
        assert!(copy.contains("origin/feature/login"));
        assert!(copy.contains("create branch from source"));
    }
}
