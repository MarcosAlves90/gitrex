use std::{
    fs,
    path::Path,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::{
    git::{CherryPickStatus, GitClient, ResetMode},
    test_support::{
        checkout_branch, commit_all, configure_user, create_branch, init_repo, write_file,
    },
};

use super::{
    app::{App, CommitActionFlow, View},
    controller::TuiController,
    operations::{GitOperationRunner, OperationOutcome, OperationRequest},
};

fn git_output(repository: &Path, arguments: &[&str]) -> Output {
    Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .unwrap()
}

fn git_success(repository: &Path, arguments: &[&str]) -> String {
    let output = git_output(repository, arguments);
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn press(controller: &mut TuiController, key: KeyCode) {
    controller
        .handle_event(Event::Key(KeyEvent::new(key, KeyModifiers::NONE)))
        .unwrap();
}

fn render_commit_actions(controller: &mut TuiController) -> String {
    let backend = ratatui::backend::TestBackend::new(90, 18);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| controller.app_mut().render(frame))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .concat()
}

fn wait_for_operation(controller: &mut TuiController) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while controller.app().loading.is_some() {
        controller.poll_operation().unwrap();
        assert!(Instant::now() < deadline, "Git operation did not finish");
        thread::sleep(Duration::from_millis(10));
    }
    controller.poll_operation().unwrap();
}

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

#[test]
fn safety_regression_cherry_pick_ignored_collision_blocks_branch_switch() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), ".gitignore", "artifact.txt\n");
    write_file(temp.path(), "README.md", "base\n");
    let base = commit_all(&repo, "base");

    create_branch(&repo, "destination", base.as_str());
    checkout_branch(&repo, "destination");
    write_file(temp.path(), "artifact.txt", "destination version\n");
    git_success(temp.path(), &["add", "-f", "artifact.txt"]);
    commit_all(&repo, "track destination artifact");

    create_branch(&repo, "source", base.as_str());
    checkout_branch(&repo, "source");
    write_file(temp.path(), "source.txt", "source change\n");
    let source = commit_all(&repo, "source change");
    checkout_branch(&repo, "main");

    write_file(temp.path(), "artifact.txt", "local ignored data\n");
    let client = GitClient::from_path(temp.path());
    assert!(client.status().unwrap().files.is_empty());

    let result = client
        .cherry_pick_to_branch(source.as_str(), "destination")
        .unwrap();

    assert_eq!(result.status, CherryPickStatus::Failed);
    assert_eq!(client.status().unwrap().branch_name, "main");
    assert_eq!(
        fs::read_to_string(temp.path().join("artifact.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "local ignored data\n"
    );
}

#[test]
fn safety_regression_cherry_pick_source_collision_blocks_before_switch() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), ".gitignore", "artifact.txt\n");
    write_file(temp.path(), "README.md", "base\n");
    let base = commit_all(&repo, "base");

    create_branch(&repo, "source", base.as_str());
    checkout_branch(&repo, "source");
    write_file(temp.path(), "artifact.txt", "source tracked version\n");
    git_success(temp.path(), &["add", "-f", "artifact.txt"]);
    let source = commit_all(&repo, "add artifact");

    create_branch(&repo, "destination", base.as_str());
    checkout_branch(&repo, "main");
    write_file(temp.path(), "artifact.txt", "local ignored data\n");
    let client = GitClient::from_path(temp.path());
    assert!(client.status().unwrap().files.is_empty());

    let result = client
        .cherry_pick_to_branch(source.as_str(), "destination")
        .unwrap();

    assert_ne!(result.status, CherryPickStatus::Applied);
    assert_eq!(client.status().unwrap().branch_name, "main");
    assert_eq!(
        fs::read_to_string(temp.path().join("artifact.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "local ignored data\n"
    );
    assert!(
        !git_output(temp.path(), &["rev-parse", "--verify", "CHERRY_PICK_HEAD"])
            .status
            .success()
    );
}

#[test]
fn safety_regression_hard_reset_worker_blocks_ignored_target_collision() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), ".gitignore", "artifact.txt\n");
    write_file(temp.path(), "README.md", "base\n");
    let base = commit_all(&repo, "base");

    create_branch(&repo, "target", base.as_str());
    checkout_branch(&repo, "target");
    write_file(temp.path(), "artifact.txt", "tracked target version\n");
    git_success(temp.path(), &["add", "-f", "artifact.txt"]);
    let target = commit_all(&repo, "track artifact");
    checkout_branch(&repo, "main");
    write_file(temp.path(), "artifact.txt", "local ignored data\n");

    let client = GitClient::from_path(temp.path());
    assert!(client.status().unwrap().files.is_empty());
    let result = client.reset_to_commit(target.as_str(), ResetMode::Hard, "main", base.as_str());

    assert!(
        result.is_err(),
        "Hard reset must refuse the ignored collision"
    );
    assert_eq!(client.resolve_commit("HEAD").unwrap(), base);
    assert_eq!(
        fs::read_to_string(temp.path().join("artifact.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "local ignored data\n"
    );
}

#[test]
fn safety_regression_hard_reset_ignored_collision_is_listed_and_blocked() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), ".gitignore", "artifact.txt\n");
    write_file(temp.path(), "README.md", "base\n");
    commit_all(&repo, "base");

    write_file(temp.path(), "artifact.txt", "tracked target version\n");
    git_success(temp.path(), &["add", "-f", "artifact.txt"]);
    let target = commit_all(&repo, "track artifact");
    fs::remove_file(temp.path().join("artifact.txt")).unwrap();
    let current = commit_all(&repo, "remove artifact");
    write_file(temp.path(), "artifact.txt", "local ignored data\n");

    let client = GitClient::from_path(temp.path());
    assert!(client.status().unwrap().files.is_empty());
    let mut controller = TuiController::new(client.clone());
    controller.refresh().unwrap();
    controller.app_mut().select_view(View::Log);
    controller.app_mut().move_commit_selection(1);
    assert_eq!(
        controller.app().selected_commit().unwrap().hash,
        target,
        "the target commit should be visible in the current branch history"
    );
    controller.app_mut().open_commit_actions();
    for _ in 0..4 {
        press(&mut controller, KeyCode::Char('j'));
    }
    press(&mut controller, KeyCode::Enter);
    press(&mut controller, KeyCode::Char('j'));
    press(&mut controller, KeyCode::Char('j'));
    press(&mut controller, KeyCode::Enter);
    press(&mut controller, KeyCode::Enter);

    assert!(matches!(
        controller.app().commit_action_flow,
        Some(CommitActionFlow::HardResetConfirmation { .. })
    ));
    let review = render_commit_actions(&mut controller);
    assert!(
        review.contains("artifact.txt"),
        "collision path hidden: {review}"
    );
    assert!(
        review.contains("blocked"),
        "Hard reset is not marked blocked: {review}"
    );

    press(&mut controller, KeyCode::Char('y'));
    assert!(controller.app().loading.is_none());
    assert_eq!(client.resolve_commit("HEAD").unwrap(), current);
    assert_eq!(
        fs::read_to_string(temp.path().join("artifact.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "local ignored data\n"
    );

    fs::remove_file(temp.path().join("artifact.txt")).unwrap();
    press(&mut controller, KeyCode::Esc);
    press(&mut controller, KeyCode::Enter);
    assert!(matches!(
        controller.app().commit_action_flow,
        Some(CommitActionFlow::HardResetConfirmation { .. })
    ));
    press(&mut controller, KeyCode::Char('y'));
    wait_for_operation(&mut controller);
    assert_eq!(client.resolve_commit("HEAD").unwrap(), target);
    assert_eq!(
        fs::read_to_string(temp.path().join("artifact.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "tracked target version\n"
    );
}

#[test]
fn safety_regression_empty_cherry_pick_is_not_conflict_and_can_be_skipped() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path(), "main");
    configure_user(&repo);
    write_file(temp.path(), "README.md", "base\n");
    let base = commit_all(&repo, "base");

    create_branch(&repo, "source", base.as_str());
    checkout_branch(&repo, "source");
    write_file(temp.path(), "source.txt", "already applied\n");
    let source = commit_all(&repo, "source change");

    create_branch(&repo, "destination", base.as_str());
    checkout_branch(&repo, "destination");
    git_success(temp.path(), &["cherry-pick", source.as_str()]);
    checkout_branch(&repo, "main");

    let client = GitClient::from_path(temp.path());
    let receiver = GitOperationRunner::new(client).spawn(OperationRequest::CherryPick {
        source,
        destination: String::from("destination"),
    });
    let outcome = receiver.recv().unwrap();
    let message = match outcome {
        OperationOutcome::StateChangedFailure { message, .. } => message,
        other => panic!("expected a stopped cherry-pick outcome, got {other:?}"),
    };

    assert!(!message.contains("Cherry-pick conflict"), "{message}");
    assert!(message.contains("git cherry-pick --skip"), "{message}");
    assert!(
        git_output(temp.path(), &["rev-parse", "--verify", "CHERRY_PICK_HEAD"])
            .status
            .success()
    );
    let unresolved = git_success(temp.path(), &["ls-files", "--unmerged"]);
    assert!(unresolved.is_empty());
    git_success(temp.path(), &["cherry-pick", "--skip"]);
    assert!(
        !git_output(temp.path(), &["rev-parse", "--verify", "CHERRY_PICK_HEAD"])
            .status
            .success()
    );
}
