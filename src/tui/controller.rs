use crossterm::event::{Event, KeyCode, KeyEvent};

use crate::{
    git::GitClient,
};

use super::{
    app::{App, CommitAction, MessageKind, PickerAction, View},
    operations::{build_snapshot, GitOperationRunner, OperationOutcome, OperationRequest},
};

pub struct TuiController {
    client: GitClient,
    app: App,
    runner: GitOperationRunner,
    operation_rx: Option<std::sync::mpsc::Receiver<OperationOutcome>>,
}

impl TuiController {
    pub fn new(client: GitClient) -> Self {
        let runner = GitOperationRunner::new(<GitClient as Clone>::clone(&client));
        Self {
            client,
            app: App::new(),
            runner,
            operation_rx: None,
        }
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let snapshot = build_snapshot(&self.client).map_err(anyhow::Error::msg)?;
        self.app.apply_snapshot(
            snapshot.status.unwrap(),
            snapshot.branches,
            snapshot.log,
            snapshot.selected_branch,
        );
        self.app.set_feedback("Repository refreshed.", MessageKind::Success);
        Ok(())
    }

    pub fn poll_operation(&mut self) -> anyhow::Result<()> {
        let Some(rx) = self.operation_rx.as_ref() else {
            return Ok(());
        };

        match rx.try_recv() {
            Ok(outcome) => {
                self.operation_rx = None;
                self.finish_operation(outcome)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.operation_rx = None;
                self.app.stop_loading();
                self.app.set_feedback("Operation aborted unexpectedly.", MessageKind::Error);
                Ok(())
            }
        }
    }

    pub fn handle_event(&mut self, event: Event) -> anyhow::Result<bool> {
        if self.operation_rx.is_some() {
            self.poll_operation()?;
        }

        let intent = match event {
            Event::Key(key) => self.intent_for_key(key),
            _ => Intent::None,
        };
        self.apply_intent(intent)
    }

    pub fn start_operation(&mut self, action: PickerAction) -> anyhow::Result<()> {
        if self.operation_rx.is_some() {
            return Ok(());
        }

        let operation = self.build_operation(action)?;
        let label = operation.loading_label();
        self.app.start_loading(label.clone());
        self.operation_rx = Some(self.runner.spawn(operation));
        self.app.set_feedback(format!("{label}..."), MessageKind::Info);
        Ok(())
    }

    fn start_branch_creation(&mut self) -> anyhow::Result<()> {
        let Some((branch, start_point)) = self.app.branch_create_request() else {
            self.app.set_feedback("Type a branch name first.", MessageKind::Warning);
            return Ok(());
        };

        let operation = OperationRequest::CreateBranch { branch, start_point };
        let label = operation.loading_label();
        self.app.close_branch_creator();
        self.app.start_loading(label.clone());
        self.operation_rx = Some(self.runner.spawn(operation));
        self.app.set_feedback(format!("{label}..."), MessageKind::Info);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    None,
    Quit,
    Refresh,
    SelectView(View),
    MoveSelection(isize),
    MoveCommitSelection(isize),
    OpenPicker,
    ClosePicker,
    OpenCommitActions,
    CloseCommitActions,
    MovePicker(isize),
    MoveCommitAction(isize),
    ConfirmPicker,
    ConfirmCommitAction,
    CancelBranchCreate,
    DeleteBranchName,
    TypeBranchName(char),
    ConfirmBranchCreate,
}

impl TuiController {
    fn intent_for_key(&self, key: KeyEvent) -> Intent {
        if self.app.branch_create_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CancelBranchCreate,
                KeyCode::Enter => Intent::ConfirmBranchCreate,
                KeyCode::Backspace => Intent::DeleteBranchName,
                KeyCode::Char(ch) => Intent::TypeBranchName(ch),
                _ => Intent::None,
            };
        }

        if self.app.picker_open {
            return match key.code {
                KeyCode::Esc => Intent::ClosePicker,
                KeyCode::Enter => Intent::ConfirmPicker,
                KeyCode::Char('j') | KeyCode::Down => Intent::MovePicker(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MovePicker(-1),
                _ => Intent::None,
            };
        }

        if self.app.commit_actions_are_open() {
            return match key.code {
                KeyCode::Esc => Intent::CloseCommitActions,
                KeyCode::Enter => Intent::ConfirmCommitAction,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveCommitAction(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveCommitAction(-1),
                _ => Intent::None,
            };
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('1') => Intent::SelectView(View::Status),
            KeyCode::Char('2') => Intent::SelectView(View::Branches),
            KeyCode::Char('3') => Intent::SelectView(View::Log),
            KeyCode::Char('j') | KeyCode::Down if matches!(self.app.view, View::Branches) => {
                Intent::MoveSelection(1)
            }
            KeyCode::Char('k') | KeyCode::Up if matches!(self.app.view, View::Branches) => {
                Intent::MoveSelection(-1)
            }
            KeyCode::Char('j') | KeyCode::Down if matches!(self.app.view, View::Log) => {
                Intent::MoveCommitSelection(1)
            }
            KeyCode::Char('k') | KeyCode::Up if matches!(self.app.view, View::Log) => {
                Intent::MoveCommitSelection(-1)
            }
            KeyCode::Enter if matches!(self.app.view, View::Branches) => Intent::OpenPicker,
            KeyCode::Enter if matches!(self.app.view, View::Log) => Intent::OpenCommitActions,
            _ => Intent::None,
        }
    }

    fn apply_intent(&mut self, intent: Intent) -> anyhow::Result<bool> {
        match intent {
            Intent::None => Ok(false),
            Intent::Quit => Ok(true),
            Intent::Refresh => {
                self.refresh()?;
                Ok(false)
            }
            Intent::SelectView(view) => {
                self.app.select_view(view);
                Ok(false)
            }
            Intent::MoveSelection(delta) => {
                self.app.move_selection(delta);
                Ok(false)
            }
            Intent::MoveCommitSelection(delta) => {
                self.app.move_commit_selection(delta);
                Ok(false)
            }
            Intent::OpenPicker => {
                self.app.open_picker();
                Ok(false)
            }
            Intent::ClosePicker => {
                self.app.close_picker();
                Ok(false)
            }
            Intent::OpenCommitActions => {
                self.app.open_commit_actions();
                Ok(false)
            }
            Intent::CloseCommitActions => {
                self.app.close_commit_actions();
                Ok(false)
            }
            Intent::MovePicker(delta) => {
                self.app.move_picker(delta);
                Ok(false)
            }
            Intent::MoveCommitAction(delta) => {
                self.app.move_commit_action(delta);
                Ok(false)
            }
            Intent::ConfirmPicker => {
                self.confirm_picker()?;
                Ok(false)
            }
            Intent::ConfirmCommitAction => {
                self.confirm_commit_action()?;
                Ok(false)
            }
            Intent::CancelBranchCreate => {
                self.app.close_branch_creator();
                Ok(false)
            }
            Intent::DeleteBranchName => {
                self.app.pop_branch_create_char();
                Ok(false)
            }
            Intent::TypeBranchName(ch) => {
                self.app.push_branch_create_char(ch);
                Ok(false)
            }
            Intent::ConfirmBranchCreate => {
                self.start_branch_creation()?;
                Ok(false)
            }
        }
    }

    fn confirm_picker(&mut self) -> anyhow::Result<()> {
        let action = *self
            .app
            .picker_actions()
            .get(self.app.picker_index)
            .unwrap_or(&PickerAction::Checkout);
        self.app.close_picker();
        match action {
            PickerAction::CreateBranch => {
                self.app.open_branch_creator();
                Ok(())
            }
            _ => self.start_operation(action),
        }
    }

    fn confirm_commit_action(&mut self) -> anyhow::Result<()> {
        let action = *self
            .app
            .commit_actions()
            .get(self.app.commit_action_index)
            .unwrap_or(&CommitAction::CheckoutCommit);
        self.app.close_commit_actions();

        match action {
            CommitAction::CheckoutCommit => {
                let target = self
                    .app
                    .selected_commit()
                    .map(|commit| commit.hash.clone())
                    .ok_or_else(|| anyhow::anyhow!("No commit selected."))?;
                self.start_operation_from_target(target)
            }
            CommitAction::CreateBranchFromCommit => {
                let source = self
                    .app
                    .selected_commit()
                    .map(|commit| commit.hash.clone())
                    .ok_or_else(|| anyhow::anyhow!("No commit selected."))?;
                self.app.open_branch_creator_from_source(source);
                Ok(())
            }
        }
    }

    fn build_operation(&self, action: PickerAction) -> anyhow::Result<OperationRequest> {
        match action {
            PickerAction::Checkout => {
                let branch = self
                    .app
                    .selected_branch()
                    .map(|branch| branch.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
                Ok(OperationRequest::Checkout { branch })
            }
            PickerAction::Switch => {
                let branch = self
                    .app
                    .selected_branch()
                    .map(|branch| branch.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
                Ok(OperationRequest::Switch { branch })
            }
            PickerAction::Pull => {
                let sync = self.app.current_sync_target();
                let (remote, branch) = sync
                    .map(|(remote, branch)| (Some(remote), Some(branch)))
                    .unwrap_or((None, None));
                Ok(OperationRequest::Pull { remote, branch })
            }
            PickerAction::Push => {
                let sync = self.app.current_sync_target();
                let (remote, branch) = sync
                    .map(|(remote, branch)| (Some(remote), Some(branch)))
                    .unwrap_or((None, None));
                Ok(OperationRequest::Push { remote, branch })
            }
            PickerAction::CreateBranch => Err(anyhow::anyhow!(
                "Create branch is handled through the input prompt."
            )),
        }
    }

    fn start_operation_from_target(&mut self, target: String) -> anyhow::Result<()> {
        let operation = OperationRequest::Checkout { branch: target };
        let label = operation.loading_label();
        self.app.start_loading(label.clone());
        self.operation_rx = Some(self.runner.spawn(operation));
        self.app.set_feedback(format!("{label}..."), MessageKind::Info);
        Ok(())
    }

    fn finish_operation(&mut self, outcome: OperationOutcome) -> anyhow::Result<()> {
        self.app.stop_loading();
        match outcome {
            OperationOutcome::Success { snapshot, message } => {
                self.app
                    .apply_snapshot(
                        snapshot.status.unwrap(),
                        snapshot.branches,
                        snapshot.log,
                        snapshot.selected_branch,
                    );
                self.app.set_feedback(message, MessageKind::Success);
            }
            OperationOutcome::Error(message) => {
                self.app.set_feedback(message, MessageKind::Error);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Intent, TuiController, View};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use crate::{domain::{BranchInfo, BranchKind, CommitSummary, RepoStatus}, git::GitClient};

    #[test]
    fn controller_routes_enter_to_picker_opening_and_log_actions() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().select_view(View::Branches);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenPicker
        ));

        controller.app_mut().select_view(View::Log);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenCommitActions
        ));
    }

    #[test]
    fn controller_handles_non_key_events_as_noop() {
        let mut controller = TuiController::new(GitClient::new());
        assert!(!controller.handle_event(Event::Resize(80, 24)).unwrap());
    }

    #[test]
    fn log_view_uses_commit_navigation_and_actions() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().log = vec![
            CommitSummary {
                hash: "abc123".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Initial commit".to_string(),
            },
            CommitSummary {
                hash: "def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Add feature".to_string(),
            },
        ];
        controller.app_mut().select_view(View::Log);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Intent::MoveCommitSelection(1)
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenCommitActions
        ));
    }

    #[test]
    fn controller_opens_branch_creator_from_picker_action() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![BranchInfo {
            name: "main".to_string(),
            current: true,
            upstream: None,
            commit: "abc".to_string(),
            subject: "init".to_string(),
            kind: BranchKind::Local,
        }];
        controller.app_mut().status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: None,
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });
        controller.app_mut().open_picker();
        controller.app_mut().picker_index = 4;

        controller.confirm_picker().unwrap();

        assert!(controller.app().branch_create_is_open());
        assert_eq!(controller.app().branch_create_source.as_deref(), Some("main"));
    }
}
