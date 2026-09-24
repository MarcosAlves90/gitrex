use std::{
    fs,
    path::Path,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use crate::git::GitClient;

use super::controller::TuiController;

fn git(repository: &Path, arguments: &[&str]) -> Output {
    Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .unwrap()
}

fn checked_git(repository: &Path, arguments: &[&str]) {
    let output = git(repository, arguments);
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn controller_with_merged_branch() -> (TempDir, TuiController) {
    let temp = tempfile::tempdir().unwrap();
    checked_git(temp.path(), &["init", "--quiet"]);
    checked_git(temp.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    checked_git(temp.path(), &["config", "user.name", "Gitrex Test"]);
    checked_git(temp.path(), &["config", "user.email", "gitrex@example.com"]);
    fs::write(temp.path().join("base.txt"), "base\n").unwrap();
    checked_git(temp.path(), &["add", "-A"]);
    checked_git(temp.path(), &["commit", "--quiet", "-m", "initial commit"]);
    checked_git(temp.path(), &["branch", "--", "feature/merged", "HEAD"]);

    let mut controller = TuiController::new(GitClient::from_path(temp.path()));
    controller.refresh().unwrap();
    (temp, controller)
}

fn key(controller: &mut TuiController, code: KeyCode) {
    controller
        .handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
        .unwrap();
}

fn render_text(controller: &mut TuiController) -> String {
    let backend = ratatui::backend::TestBackend::new(120, 40);
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

#[test]
fn cleanup_modal_shows_candidates_and_supports_selection_and_cancel_keys() {
    let (temp, mut controller) = controller_with_merged_branch();

    key(&mut controller, KeyCode::Char('c'));
    let initial = render_text(&mut controller);
    assert!(
        initial.contains("Cleanup merged local branches"),
        "cleanup modal did not open: {initial}"
    );
    assert!(
        initial.contains("feature/merged"),
        "candidate missing: {initial}"
    );
    assert!(
        initial.contains("[x] feature/merged"),
        "candidate not selected: {initial}"
    );

    key(&mut controller, KeyCode::Char(' '));
    assert!(
        render_text(&mut controller).contains("[ ] feature/merged"),
        "Space did not toggle candidate selection"
    );
    key(&mut controller, KeyCode::Char('a'));
    assert!(
        render_text(&mut controller).contains("[x] feature/merged"),
        "a did not select all candidates"
    );
    key(&mut controller, KeyCode::Char('n'));
    assert!(
        render_text(&mut controller).contains("[ ] feature/merged"),
        "n did not clear candidate selection"
    );
    key(&mut controller, KeyCode::Char('a'));
    key(&mut controller, KeyCode::Enter);
    let warning = render_text(&mut controller);
    assert!(
        warning.contains("Confirm cleanup"),
        "warning did not open: {warning}"
    );
    assert!(
        warning.contains("feature/merged"),
        "warning omitted selected branch: {warning}"
    );
    assert!(
        warning.contains("safe branch deletion"),
        "warning omitted safety detail: {warning}"
    );
    key(&mut controller, KeyCode::Esc);
    assert!(
        render_text(&mut controller).contains("Cleanup merged local branches"),
        "Escape did not return to cleanup selection"
    );
    key(&mut controller, KeyCode::Esc);

    let after_cancel = render_text(&mut controller);
    assert!(
        !after_cancel.contains("Cleanup merged local branches"),
        "Escape left cleanup modal open: {after_cancel}"
    );
    assert!(
        git(
            temp.path(),
            &[
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/feature/merged"
            ]
        )
        .status
        .success(),
        "Escape deleted the selected branch"
    );
}

#[test]
fn cleanup_confirmation_revalidates_each_branch_reports_partial_results_and_refreshes() {
    let (temp, mut controller) = controller_with_merged_branch();
    checked_git(temp.path(), &["branch", "feature/stale", "HEAD"]);

    key(&mut controller, KeyCode::Char('c'));
    key(&mut controller, KeyCode::Enter);
    let warning = render_text(&mut controller);
    assert!(warning.contains("feature/merged"));
    assert!(warning.contains("feature/stale"));

    checked_git(temp.path(), &["branch", "feature/work", "HEAD"]);
    checked_git(temp.path(), &["checkout", "--quiet", "feature/work"]);
    fs::write(temp.path().join("stale.txt"), "unmerged commit\n").unwrap();
    checked_git(temp.path(), &["add", "stale.txt"]);
    checked_git(temp.path(), &["commit", "--quiet", "-m", "unmerged work"]);
    checked_git(temp.path(), &["branch", "--force", "feature/stale", "HEAD"]);
    checked_git(temp.path(), &["checkout", "--quiet", "main"]);

    key(&mut controller, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !controller.app().cleanup_report_is_open() {
        controller.poll_operation().unwrap();
        assert!(
            Instant::now() < deadline,
            "cleanup operation did not finish"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let report = render_text(&mut controller);
    assert!(
        report.contains("Cleanup results"),
        "cleanup report did not open: {report}"
    );
    assert!(
        report.contains("1 deleted, 1 skipped, 0 failed"),
        "partial summary missing: {report}"
    );
    assert!(
        report.contains("deleted: feature/merged"),
        "deleted branch result missing: {report}"
    );
    assert!(
        report.contains("skipped: feature/stale"),
        "stale branch result missing: {report}"
    );
    assert!(!controller
        .app()
        .branches
        .iter()
        .any(|branch| branch.name == "feature/merged"));
    assert!(controller
        .app()
        .branches
        .iter()
        .any(|branch| branch.name == "feature/stale"));
}
