use crossterm::event::{Event, KeyCode, KeyEvent};

use crate::{
    git::GitClient,
};

use super::{
    app::{App, MessageKind, PickerAction, View},
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
    OpenPicker,
    ClosePicker,
    MovePicker(isize),
    ConfirmPicker,
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

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('1') => Intent::SelectView(View::Status),
            KeyCode::Char('2') => Intent::SelectView(View::Branches),
            KeyCode::Char('3') => Intent::SelectView(View::Log),
            KeyCode::Char('j') | KeyCode::Down => Intent::MoveSelection(1),
            KeyCode::Char('k') | KeyCode::Up => Intent::MoveSelection(-1),
            KeyCode::Enter => Intent::OpenPicker,
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
            Intent::OpenPicker => {
                self.app.open_picker();
                Ok(false)
            }
            Intent::ClosePicker => {
                self.app.close_picker();
                Ok(false)
            }
            Intent::MovePicker(delta) => {
                self.app.move_picker(delta);
                Ok(false)
            }
            Intent::ConfirmPicker => {
                self.confirm_picker()?;
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
    use super::{Intent, TuiController};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use crate::{
        domain::{BranchInfo, BranchKind, RepoStatus},
        git::GitClient,
    };

    #[test]
    fn controller_routes_enter_to_picker_opening() {
        let controller = TuiController::new(GitClient::new());
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenPicker
        ));
    }

    #[test]
    fn controller_handles_non_key_events_as_noop() {
        let mut controller = TuiController::new(GitClient::new());
        assert!(!controller.handle_event(Event::Resize(80, 24)).unwrap());
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
