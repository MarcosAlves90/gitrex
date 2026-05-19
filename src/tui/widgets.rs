use super::app::View;

pub fn mode_label(view: View) -> &'static str {
    match view {
        View::Status => "status",
        View::Branches => "branches",
        View::Log => "log",
    }
}

pub fn actions_copy(selected_branch: Option<&str>) -> String {
    let branch = selected_branch.unwrap_or("no branch selected");
    [
        "Keys:",
        "j/k or arrows = move",
        "1/2/3 = change pane",
        "Enter/c = checkout branch",
        "s = switch branch",
        "p = pull current branch",
        "P = push current branch",
        "r = refresh",
        "q = quit",
        "",
        "Selected branch:",
        branch,
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
        assert_eq!(mode_label(View::Log), "log");
    }

    #[test]
    fn actions_copy_mentions_selected_branch() {
        let copy = actions_copy(Some("feature/login"));
        assert!(copy.contains("feature/login"));
        assert!(copy.contains("Enter/c = checkout branch"));
    }
}
