use super::app::App;

#[test]
fn commit_action_menu_exposes_new_actions() {
    let app = App::new();
    let labels = app
        .commit_actions()
        .iter()
        .map(|action| action.label())
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "checkout commit",
            "create branch from commit",
            "compare commit",
            "cherry-pick commit",
            "reset current branch",
        ]
    );
}
