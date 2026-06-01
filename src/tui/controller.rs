use crossterm::event::{Event, KeyCode, KeyEvent};

use crate::git::GitClient;

use super::{
    app::{
        App, BranchPanel, CommitAction, DeleteBranchTarget, MessageKind, PickerAction,
        RemoteBranchAction, View,
    },
    operation_flow,
    operations::{GitOperationRunner, OperationOutcome, OperationRequest},
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

    #[cfg(test)]
    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let snapshot = self.client.snapshot().map_err(anyhow::Error::msg)?;
        self.app.apply_snapshot(snapshot);
        self.app
            .set_feedback("Repository refreshed.", MessageKind::Success);
        Ok(())
    }

    pub fn poll_operation(&mut self) -> anyhow::Result<()> {
        let Some(rx) = self.operation_rx.as_ref() else {
            return Ok(());
        };

        match rx.try_recv() {
            Ok(outcome) => {
                self.operation_rx = None;
                operation_flow::finish_operation(&mut self.app, outcome)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.operation_rx = None;
                self.app.stop_loading();
                self.app
                    .set_feedback("Operation aborted unexpectedly.", MessageKind::Error);
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

    pub fn tick(&mut self) {
        self.app.advance_loading_frame();
        if matches!(self.app.view, View::Log) {
            self.app.advance_graph_scroll();
        }
    }

    pub fn start_operation(&mut self, action: PickerAction) -> anyhow::Result<()> {
        let operation = self.build_operation(action)?;
        self.start_operation_request(operation)
    }

    fn start_branch_creation(&mut self) -> anyhow::Result<()> {
        let Some((branch, start_point)) = self.app.branch_create_request() else {
            self.app
                .set_feedback("Type a branch name first.", MessageKind::Warning);
            return Ok(());
        };

        let operation = OperationRequest::CreateBranch {
            branch,
            start_point,
        };
        self.app.close_branch_creator();
        self.start_operation_request(operation)?;
        Ok(())
    }

    fn start_detached_checkout(&mut self, target: String) -> anyhow::Result<()> {
        let operation = OperationRequest::CheckoutDetached { target };
        self.start_operation_request(operation)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    None,
    Quit,
    Refresh,
    SelectView(View),
    ToggleBranchPanel,
    MoveSelection(isize),
    MoveCommitSelection(isize),
    MoveHelpScroll(isize),
    OpenBranchSearch,
    CloseBranchSearch,
    DeleteBranchSearchChar,
    TypeBranchSearchChar(char),
    ConfirmBranchSearch,
    OpenPicker,
    OpenRemotePicker,
    ClosePicker,
    CloseRemotePicker,
    OpenCommitActions,
    CloseCommitActions,
    MovePicker(isize),
    MoveRemotePicker(isize),
    MoveCommitAction(isize),
    ConfirmPicker,
    ConfirmRemotePicker,
    ConfirmCommitAction,
    ConfirmDeleteBranch,
    CancelDeleteBranch,
    OpenHelp,
    CloseHelp,
    CancelBranchCreate,
    DeleteBranchName,
    TypeBranchName(char),
    ConfirmBranchCreate,
}

impl TuiController {
    fn intent_for_key(&self, key: KeyEvent) -> Intent {
        if self.app.help_is_open() {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('h') => Intent::CloseHelp,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveHelpScroll(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveHelpScroll(-1),
                KeyCode::Char('q') => Intent::Quit,
                _ => Intent::None,
            };
        }

        if self.app.delete_branch_confirm_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CancelDeleteBranch,
                KeyCode::Enter => Intent::ConfirmDeleteBranch,
                _ => Intent::None,
            };
        }

        if self.app.branch_create_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CancelBranchCreate,
                KeyCode::Enter => Intent::ConfirmBranchCreate,
                KeyCode::Backspace => Intent::DeleteBranchName,
                KeyCode::Char(ch) => Intent::TypeBranchName(ch),
                _ => Intent::None,
            };
        }

        if self.app.branch_search_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CloseBranchSearch,
                KeyCode::Enter => Intent::ConfirmBranchSearch,
                KeyCode::Backspace => Intent::DeleteBranchSearchChar,
                KeyCode::Char(ch) => Intent::TypeBranchSearchChar(ch),
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

        if self.app.remote_picker_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CloseRemotePicker,
                KeyCode::Enter => Intent::ConfirmRemotePicker,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveRemotePicker(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveRemotePicker(-1),
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
            KeyCode::Tab if matches!(self.app.view, View::Branches) => Intent::ToggleBranchPanel,
            KeyCode::BackTab if matches!(self.app.view, View::Branches) => {
                Intent::ToggleBranchPanel
            }
            KeyCode::Char('j') | KeyCode::Down if matches!(self.app.view, View::Log) => {
                Intent::MoveCommitSelection(1)
            }
            KeyCode::Char('k') | KeyCode::Up if matches!(self.app.view, View::Log) => {
                Intent::MoveCommitSelection(-1)
            }
            KeyCode::Char('/') if matches!(self.app.view, View::Branches) => {
                Intent::OpenBranchSearch
            }
            KeyCode::Char('h') => Intent::OpenHelp,
            KeyCode::Enter if matches!(self.app.view, View::Branches) => {
                match self.app.branch_panel() {
                    BranchPanel::Local => Intent::OpenPicker,
                    BranchPanel::Remote => Intent::OpenRemotePicker,
                }
            }
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
            Intent::ToggleBranchPanel => {
                self.app.toggle_branch_panel();
                if let Some(history) =
                    operation_flow::refresh_selected_branch_history(&self.app, &self.client)?
                {
                    self.app.apply_graph_history(history);
                }
                Ok(false)
            }
            Intent::MoveSelection(delta) => {
                self.app.move_selection(delta);
                if let Some(history) =
                    operation_flow::refresh_selected_branch_history(&self.app, &self.client)?
                {
                    self.app.apply_graph_history(history);
                }
                Ok(false)
            }
            Intent::MoveCommitSelection(delta) => {
                self.app.move_commit_selection(delta);
                Ok(false)
            }
            Intent::MoveHelpScroll(delta) => {
                self.app.move_help_scroll(delta);
                Ok(false)
            }
            Intent::OpenBranchSearch => {
                self.app.open_branch_search();
                Ok(false)
            }
            Intent::CloseBranchSearch => {
                self.app.close_branch_search();
                Ok(false)
            }
            Intent::DeleteBranchSearchChar => {
                self.app.pop_branch_search_char();
                Ok(false)
            }
            Intent::TypeBranchSearchChar(ch) => {
                self.app.push_branch_search_char(ch);
                Ok(false)
            }
            Intent::ConfirmBranchSearch => {
                if self.app.confirm_branch_search() {
                    if let Some(history) =
                        operation_flow::refresh_selected_branch_history(&self.app, &self.client)?
                    {
                        self.app.apply_graph_history(history);
                    }
                }
                Ok(false)
            }
            Intent::OpenPicker => {
                self.app.open_picker();
                Ok(false)
            }
            Intent::OpenRemotePicker => {
                self.app.open_remote_picker();
                Ok(false)
            }
            Intent::ClosePicker => {
                self.app.close_picker();
                Ok(false)
            }
            Intent::CloseRemotePicker => {
                self.app.close_remote_picker();
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
            Intent::MoveRemotePicker(delta) => {
                self.app.move_remote_picker(delta);
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
            Intent::ConfirmRemotePicker => {
                self.confirm_remote_picker()?;
                Ok(false)
            }
            Intent::ConfirmCommitAction => {
                self.confirm_commit_action()?;
                Ok(false)
            }
            Intent::ConfirmDeleteBranch => {
                self.confirm_delete_branch()?;
                Ok(false)
            }
            Intent::CancelDeleteBranch => {
                self.app.close_delete_branch_confirm();
                Ok(false)
            }
            Intent::OpenHelp => {
                self.app.open_help();
                Ok(false)
            }
            Intent::CloseHelp => {
                self.app.close_help();
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
            PickerAction::DeleteBranch => {
                let branch = self
                    .app
                    .selected_branch()
                    .map(|branch| branch.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
                self.app
                    .open_delete_branch_confirm(DeleteBranchTarget::Local { branch });
                Ok(())
            }
            _ => self.start_operation(action),
        }
    }

    fn confirm_remote_picker(&mut self) -> anyhow::Result<()> {
        let action = self
            .app
            .selected_remote_action()
            .unwrap_or(RemoteBranchAction::CreateLocalBranch);
        self.app.close_remote_picker();

        let Some(branch) = self.app.selected_remote_branch() else {
            self.app
                .set_feedback("No remote branch selected.", MessageKind::Warning);
            return Ok(());
        };

        match action {
            RemoteBranchAction::CreateLocalBranch => {
                self.app
                    .open_branch_creator_from_source(branch.full_ref(), crate::tui::theme::TEAL);
                Ok(())
            }
            RemoteBranchAction::DeleteRemoteBranch => {
                self.app
                    .open_delete_branch_confirm(DeleteBranchTarget::Remote {
                        remote: branch.remote_name().unwrap_or("remote").to_string(),
                        branch: branch.branch_short_name().to_string(),
                    });
                Ok(())
            }
            RemoteBranchAction::CheckoutDetached => self.start_detached_checkout(branch.full_ref()),
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
                self.app
                    .open_branch_creator_from_source(source, crate::tui::theme::PURPLE);
                Ok(())
            }
        }
    }

    fn confirm_delete_branch(&mut self) -> anyhow::Result<()> {
        let target = self
            .app
            .delete_branch_target()
            .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
        self.app.close_delete_branch_confirm();

        match target {
            DeleteBranchTarget::Local { branch } => {
                self.start_operation_request(OperationRequest::DeleteLocalBranch { branch })
            }
            DeleteBranchTarget::Remote { remote, branch } => self
                .start_operation_request(OperationRequest::DeleteRemoteBranch { remote, branch }),
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
            PickerAction::DeleteBranch => Err(anyhow::anyhow!(
                "Delete branch is handled through the confirmation dialog."
            )),
        }
    }

    fn start_operation_from_target(&mut self, target: String) -> anyhow::Result<()> {
        let operation = OperationRequest::Checkout { branch: target };
        self.start_operation_request(operation)
    }

    fn start_operation_request(&mut self, operation: OperationRequest) -> anyhow::Result<()> {
        operation_flow::begin_operation(
            &mut self.app,
            &self.runner,
            &mut self.operation_rx,
            operation,
        );
        Ok(())
    }

    #[cfg(test)]
    fn refresh_selected_branch_history(&mut self) -> anyhow::Result<()> {
        if let Some(history) =
            operation_flow::refresh_selected_branch_history(&self.app, &self.client)?
        {
            self.app.apply_graph_history(history);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DeleteBranchTarget, Intent, TuiController, View};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use crate::test_support::{
        checkout_branch, commit_all, configure_user, create_branch, current_dir_lock, init_repo,
        write_file, CurrentDirGuard,
    };
    use crate::{
        domain::{BranchInfo, BranchKind, CommitSummary, RepoStatus},
        git::GitClient,
    };

    #[test]
    fn controller_routes_enter_to_picker_opening_and_log_actions() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().select_view(View::Branches);
        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Local);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenPicker
        ));

        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenRemotePicker
        ));

        controller.app_mut().select_view(View::Log);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenCommitActions
        ));
    }

    #[test]
    fn controller_routes_h_to_help_and_close_it() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().select_view(View::Branches);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Intent::OpenHelp
        ));

        controller.app_mut().open_help();
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Intent::CloseHelp
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Intent::CloseHelp
        ));
    }

    #[test]
    fn controller_routes_scroll_keys_in_help() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().open_help();

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Intent::MoveHelpScroll(1)
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Intent::MoveHelpScroll(1)
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Intent::MoveHelpScroll(-1)
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Intent::MoveHelpScroll(-1)
        ));
    }

    #[test]
    fn controller_routes_tab_between_branch_panels() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().select_view(View::Branches);

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Intent::ToggleBranchPanel
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Intent::ToggleBranchPanel
        ));
    }

    #[test]
    fn controller_routes_branch_search_input_when_open() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().open_branch_search();

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
            Intent::TypeBranchSearchChar('f')
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::ConfirmBranchSearch
        ));
    }

    #[test]
    fn controller_routes_confirmation_keys_when_delete_dialog_is_open() {
        let mut controller = TuiController::new(GitClient::new());
        controller
            .app_mut()
            .open_delete_branch_confirm(DeleteBranchTarget::Local {
                branch: "main".to_string(),
            });

        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::ConfirmDeleteBranch
        ));
        assert!(matches!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Intent::CancelDeleteBranch
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
        controller.app_mut().selected_branch = Some("main".to_string());
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
        assert_eq!(
            controller.app().branch_create_source.as_deref(),
            Some("main")
        );
        assert_eq!(
            controller.app().branch_create_accent,
            crate::tui::theme::SUCCESS
        );
    }

    #[test]
    fn controller_opens_delete_confirmation_from_picker_action() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![BranchInfo {
            name: "main".to_string(),
            current: true,
            upstream: None,
            commit: "abc".to_string(),
            subject: "init".to_string(),
            kind: BranchKind::Local,
        }];
        controller.app_mut().selected_branch = Some("main".to_string());
        controller.app_mut().open_picker();
        controller.app_mut().picker_index = 5;

        controller.confirm_picker().unwrap();

        assert!(controller.app().delete_branch_confirm_is_open());
        assert_eq!(
            controller.app().delete_branch_target(),
            Some(DeleteBranchTarget::Local {
                branch: "main".to_string()
            })
        );
    }

    #[test]
    fn controller_opens_branch_creator_from_remote_branch() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);
        controller.app_mut().selected_remote_branch = Some("refs/remotes/origin/main".to_string());
        controller.app_mut().open_remote_picker();

        controller.app_mut().remote_picker_index = 0;
        controller.confirm_remote_picker().unwrap();

        assert!(controller.app().branch_create_is_open());
        assert_eq!(
            controller.app().branch_create_source.as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(
            controller.app().branch_create_accent,
            crate::tui::theme::TEAL
        );
    }

    #[test]
    fn controller_opens_delete_confirmation_from_remote_branch() {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);
        controller.app_mut().selected_remote_branch = Some("refs/remotes/origin/main".to_string());
        controller.app_mut().open_remote_picker();

        controller.app_mut().remote_picker_index = 2;
        controller.confirm_remote_picker().unwrap();

        assert!(controller.app().delete_branch_confirm_is_open());
        assert_eq!(
            controller.app().delete_branch_target(),
            Some(DeleteBranchTarget::Remote {
                remote: "origin".to_string(),
                branch: "main".to_string()
            })
        );
    }

    #[test]
    fn controller_remote_selection_refreshes_graph_for_remote_ref() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base commit");
        create_branch(&repo, "feature/login", "HEAD");
        checkout_branch(&repo, "feature/login");
        write_file(temp.path(), "README.md", "feature work\n");
        let feature_oid = commit_all(&repo, "feature work");
        repo.reference(
            "refs/remotes/origin/feature/login",
            feature_oid,
            true,
            "set remote ref",
        )
        .unwrap();
        checkout_branch(&repo, "main");
        let _restore = CurrentDirGuard::push(temp.path());

        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "main".to_string(),
                subject: "main".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "main".to_string(),
                subject: "main".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "origin/feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "feature".to_string(),
                subject: "feature".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);
        controller.app_mut().selected_remote_branch =
            Some("refs/remotes/origin/feature/login".to_string());

        controller.refresh_selected_branch_history().unwrap();

        assert!(controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "feature work"));
        assert!(!controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "main work"));
    }

    #[test]
    fn moving_branch_selection_refreshes_graph_for_selected_branch() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base commit");
        create_branch(&repo, "feature/login", "HEAD");
        checkout_branch(&repo, "feature/login");
        write_file(temp.path(), "README.md", "feature work\n");
        commit_all(&repo, "feature work");
        checkout_branch(&repo, "main");
        write_file(temp.path(), "README.md", "main work\n");
        commit_all(&repo, "main work");
        let _restore = CurrentDirGuard::push(temp.path());

        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: None,
                commit: "main".to_string(),
                subject: "main".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "feature".to_string(),
                subject: "feature".to_string(),
                kind: BranchKind::Local,
            },
        ];
        controller.app_mut().selected_branch = Some("main".to_string());
        controller.app_mut().status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: None,
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });

        controller.refresh_selected_branch_history().unwrap();
        assert!(controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "main work"));
        assert!(!controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "feature work"));

        controller.app_mut().move_selection(1);
        controller.refresh_selected_branch_history().unwrap();
        assert!(controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "feature work"));
        assert!(!controller
            .app()
            .log
            .iter()
            .any(|entry| entry.subject == "main work"));
    }
}
