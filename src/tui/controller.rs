use crossterm::event::{Event, KeyCode, KeyEvent};

use crate::git::GitClient;

use super::{
    app::{
        App, BranchPanel, CommitAction, CommitActionFlow, DeleteBranchTarget, MessageKind,
        PickerAction, RemoteBranchAction, ResetReview, View,
    },
    operation_flow,
    operations::{
        parse_cleanup_report, CleanupReport, GitOperationRunner, OperationOutcome, OperationRequest,
    },
};

pub struct TuiController {
    client: GitClient,
    app: App,
    runner: GitOperationRunner,
    operation_rx: Option<std::sync::mpsc::Receiver<OperationOutcome>>,
}

fn status_has_unresolved_conflicts(status: &crate::domain::RepoStatus) -> bool {
    status
        .files
        .iter()
        .any(|entry| entry.code.contains('U') || matches!(entry.code.as_str(), "AA" | "DD"))
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
                match outcome {
                    OperationOutcome::Success { snapshot, message } => {
                        if let Some(report) = parse_cleanup_report(&message) {
                            self.app.apply_snapshot(snapshot);
                            self.finish_cleanup(report, None);
                            Ok(())
                        } else {
                            operation_flow::finish_operation(
                                &mut self.app,
                                OperationOutcome::Success { snapshot, message },
                            )
                        }
                    }
                    OperationOutcome::SuccessWithRefreshWarning { message, warning } => {
                        if let Some(report) = parse_cleanup_report(&message) {
                            self.finish_cleanup(report, Some(warning));
                            Ok(())
                        } else {
                            operation_flow::finish_operation(
                                &mut self.app,
                                OperationOutcome::SuccessWithRefreshWarning { message, warning },
                            )
                        }
                    }
                    outcome => operation_flow::finish_operation(&mut self.app, outcome),
                }
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
    MoveCommitPage(isize),
    SelectFirstCommit,
    SelectLastCommit,
    MoveGraphHorizontal(isize),
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
    BackToCommitActionMenu,
    TypeCompareTarget(char),
    DeleteCompareTargetChar,
    ConfirmCompareTarget,
    MoveCommitActionScroll(isize),
    MoveCommitActionScrollPage(isize),
    MoveCherryPickTarget(isize),
    ConfirmCherryPickTarget,
    BackToCherryPickTargets,
    ConfirmCherryPick,
    MoveResetMode(isize),
    ConfirmResetMode,
    BackToResetModePicker,
    ConfirmReset,
    BackToResetConfirmation,
    ConfirmHardReset,
    ConfirmDeleteBranch,
    CancelDeleteBranch,
    OpenHelp,
    CloseHelp,
    CancelBranchCreate,
    DeleteBranchName,
    TypeBranchName(char),
    ConfirmBranchCreate,
    OpenCleanup,
    CloseCleanup,
    MoveCleanupSelection(isize),
    MoveCleanupScroll(isize),
    MoveCleanupScrollPage(isize),
    ToggleCleanupSelection,
    SelectAllCleanup,
    ClearCleanupSelection,
    ReviewCleanup,
    BackCleanupConfirmation,
    ConfirmCleanup,
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

        if self.app.cleanup_report_is_open() {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => Intent::CloseCleanup,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveCleanupScroll(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveCleanupScroll(-1),
                KeyCode::PageDown => Intent::MoveCleanupScrollPage(1),
                KeyCode::PageUp => Intent::MoveCleanupScrollPage(-1),
                _ => Intent::None,
            };
        }

        if self.app.cleanup_confirmation_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::BackCleanupConfirmation,
                KeyCode::Enter => Intent::ConfirmCleanup,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveCleanupScroll(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveCleanupScroll(-1),
                KeyCode::PageDown => Intent::MoveCleanupScrollPage(1),
                KeyCode::PageUp => Intent::MoveCleanupScrollPage(-1),
                _ => Intent::None,
            };
        }

        if self.app.cleanup_modal_is_open() {
            return match key.code {
                KeyCode::Esc => Intent::CloseCleanup,
                KeyCode::Enter => Intent::ReviewCleanup,
                KeyCode::Char(' ') => Intent::ToggleCleanupSelection,
                KeyCode::Char('a') => Intent::SelectAllCleanup,
                KeyCode::Char('n') => Intent::ClearCleanupSelection,
                KeyCode::Char('j') | KeyCode::Down => Intent::MoveCleanupSelection(1),
                KeyCode::Char('k') | KeyCode::Up => Intent::MoveCleanupSelection(-1),
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
            return match self.app.commit_action_flow.as_ref() {
                Some(CommitActionFlow::Menu { .. }) => match key.code {
                    KeyCode::Esc => Intent::CloseCommitActions,
                    KeyCode::Enter => Intent::ConfirmCommitAction,
                    KeyCode::Char('j') | KeyCode::Down => Intent::MoveCommitAction(1),
                    KeyCode::Char('k') | KeyCode::Up => Intent::MoveCommitAction(-1),
                    _ => Intent::None,
                },
                Some(CommitActionFlow::CompareTarget { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToCommitActionMenu,
                    KeyCode::Enter => Intent::ConfirmCompareTarget,
                    KeyCode::Backspace => Intent::DeleteCompareTargetChar,
                    KeyCode::Char(character) => Intent::TypeCompareTarget(character),
                    _ => Intent::None,
                },
                Some(CommitActionFlow::CompareResult { .. }) => match key.code {
                    KeyCode::Esc | KeyCode::Enter => Intent::BackToCommitActionMenu,
                    KeyCode::Char('j') | KeyCode::Down => Intent::MoveCommitActionScroll(1),
                    KeyCode::Char('k') | KeyCode::Up => Intent::MoveCommitActionScroll(-1),
                    KeyCode::PageDown => Intent::MoveCommitActionScrollPage(1),
                    KeyCode::PageUp => Intent::MoveCommitActionScrollPage(-1),
                    _ => Intent::None,
                },
                Some(CommitActionFlow::CherryPickTarget { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToCommitActionMenu,
                    KeyCode::Enter => Intent::ConfirmCherryPickTarget,
                    KeyCode::Char('j') | KeyCode::Down => Intent::MoveCherryPickTarget(1),
                    KeyCode::Char('k') | KeyCode::Up => Intent::MoveCherryPickTarget(-1),
                    _ => Intent::None,
                },
                Some(CommitActionFlow::CherryPickConfirmation { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToCherryPickTargets,
                    KeyCode::Enter => Intent::ConfirmCherryPick,
                    _ => Intent::None,
                },
                Some(CommitActionFlow::ResetModePicker { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToCommitActionMenu,
                    KeyCode::Enter => Intent::ConfirmResetMode,
                    KeyCode::Char('j') | KeyCode::Down => Intent::MoveResetMode(1),
                    KeyCode::Char('k') | KeyCode::Up => Intent::MoveResetMode(-1),
                    _ => Intent::None,
                },
                Some(CommitActionFlow::ResetConfirmation { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToResetModePicker,
                    KeyCode::Enter => Intent::ConfirmReset,
                    _ => Intent::None,
                },
                Some(CommitActionFlow::HardResetConfirmation { .. }) => match key.code {
                    KeyCode::Esc => Intent::BackToResetConfirmation,
                    KeyCode::Char('y') => Intent::ConfirmHardReset,
                    KeyCode::Char('j') | KeyCode::Down => Intent::MoveCommitActionScroll(1),
                    KeyCode::Char('k') | KeyCode::Up => Intent::MoveCommitActionScroll(-1),
                    KeyCode::PageDown => Intent::MoveCommitActionScrollPage(1),
                    KeyCode::PageUp => Intent::MoveCommitActionScrollPage(-1),
                    _ => Intent::None,
                },
                None => Intent::None,
            };
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('1') => Intent::SelectView(View::Branches),
            KeyCode::Char('2') => Intent::SelectView(View::Log),
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
            KeyCode::PageDown if matches!(self.app.view, View::Log) => Intent::MoveCommitPage(1),
            KeyCode::PageUp if matches!(self.app.view, View::Log) => Intent::MoveCommitPage(-1),
            KeyCode::Home | KeyCode::Char('g') if matches!(self.app.view, View::Log) => {
                Intent::SelectFirstCommit
            }
            KeyCode::End | KeyCode::Char('G') if matches!(self.app.view, View::Log) => {
                Intent::SelectLastCommit
            }
            KeyCode::Left if matches!(self.app.view, View::Log) => Intent::MoveGraphHorizontal(-4),
            KeyCode::Right if matches!(self.app.view, View::Log) => Intent::MoveGraphHorizontal(4),
            KeyCode::Char('/') if matches!(self.app.view, View::Branches) => {
                Intent::OpenBranchSearch
            }
            KeyCode::Char('h') => Intent::OpenHelp,
            KeyCode::Char('c') => Intent::OpenCleanup,
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
            Intent::MoveCommitPage(direction) => {
                self.app.move_commit_page(direction);
                Ok(false)
            }
            Intent::SelectFirstCommit => {
                self.app.select_first_commit();
                Ok(false)
            }
            Intent::SelectLastCommit => {
                self.app.select_last_commit();
                Ok(false)
            }
            Intent::MoveGraphHorizontal(delta) => {
                self.app.move_graph_horizontal(delta);
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
            Intent::BackToCommitActionMenu => {
                self.app.back_to_commit_action_menu();
                Ok(false)
            }
            Intent::TypeCompareTarget(character) => {
                self.app.type_compare_target_char(character);
                Ok(false)
            }
            Intent::DeleteCompareTargetChar => {
                self.app.delete_compare_target_char();
                Ok(false)
            }
            Intent::ConfirmCompareTarget => {
                if let Some((source, target)) = self.app.compare_target_request() {
                    self.start_operation_request(OperationRequest::CompareCommits {
                        left_reference: source,
                        right_reference: target,
                    })?;
                } else {
                    self.app.set_feedback(
                        "Enter a commit or branch reference before comparing.",
                        MessageKind::Warning,
                    );
                }
                Ok(false)
            }
            Intent::MoveCommitActionScroll(delta) => {
                self.app.move_commit_action_scroll(delta);
                Ok(false)
            }
            Intent::MoveCommitActionScrollPage(direction) => {
                self.app.move_commit_action_scroll_page(direction);
                Ok(false)
            }
            Intent::MoveCherryPickTarget(delta) => {
                self.app.move_cherry_pick_target(delta);
                Ok(false)
            }
            Intent::ConfirmCherryPickTarget => {
                if !self.app.open_cherry_pick_confirmation() {
                    self.app.set_feedback(
                        "Select a local destination branch first.",
                        MessageKind::Warning,
                    );
                }
                Ok(false)
            }
            Intent::BackToCherryPickTargets => {
                self.app.back_to_cherry_pick_targets();
                Ok(false)
            }
            Intent::ConfirmCherryPick => {
                self.start_confirmed_cherry_pick()?;
                Ok(false)
            }
            Intent::MoveResetMode(delta) => {
                self.app.move_reset_mode(delta);
                Ok(false)
            }
            Intent::ConfirmResetMode => {
                if !self.app.open_reset_confirmation() {
                    self.app
                        .set_feedback("Choose a reset mode first.", MessageKind::Warning);
                }
                Ok(false)
            }
            Intent::BackToResetModePicker => {
                self.app.back_to_reset_mode_picker();
                Ok(false)
            }
            Intent::ConfirmReset => {
                if self
                    .app
                    .reset_request()
                    .is_some_and(|(_, mode, _, _)| mode == crate::git::ResetMode::Hard)
                {
                    if !self.app.open_hard_reset_confirmation() {
                        self.app.set_feedback(
                            "Hard reset requires its separate destructive confirmation.",
                            MessageKind::Warning,
                        );
                    }
                } else {
                    self.start_confirmed_reset()?;
                }
                Ok(false)
            }
            Intent::BackToResetConfirmation => {
                self.app.back_to_reset_confirmation();
                Ok(false)
            }
            Intent::ConfirmHardReset => {
                self.start_confirmed_reset()?;
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
            Intent::OpenCleanup => {
                self.open_cleanup_modal()?;
                Ok(false)
            }
            Intent::CloseCleanup => {
                self.app.close_cleanup_modal();
                Ok(false)
            }
            Intent::MoveCleanupSelection(delta) => {
                self.app.move_cleanup_selection(delta);
                Ok(false)
            }
            Intent::MoveCleanupScroll(delta) => {
                self.app.move_cleanup_scroll(delta);
                Ok(false)
            }
            Intent::MoveCleanupScrollPage(direction) => {
                self.app.move_cleanup_scroll_page(direction);
                Ok(false)
            }
            Intent::ToggleCleanupSelection => {
                self.app.toggle_cleanup_selection();
                Ok(false)
            }
            Intent::SelectAllCleanup => {
                self.app.select_all_cleanup_candidates();
                Ok(false)
            }
            Intent::ClearCleanupSelection => {
                self.app.clear_cleanup_selection();
                Ok(false)
            }
            Intent::ReviewCleanup => {
                self.app.begin_cleanup_confirmation();
                Ok(false)
            }
            Intent::BackCleanupConfirmation => {
                self.app.close_cleanup_confirmation();
                Ok(false)
            }
            Intent::ConfirmCleanup => {
                self.confirm_cleanup()?;
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
        let Some((action, source)) = self.app.selected_commit_action() else {
            self.app
                .set_feedback("No commit action is selected.", MessageKind::Warning);
            return Ok(());
        };

        match action {
            CommitAction::CheckoutCommit => {
                self.app.close_commit_actions();
                self.start_operation_from_target(source)
            }
            CommitAction::CreateBranchFromCommit => {
                self.app.close_commit_actions();
                self.app
                    .open_branch_creator_from_source(source, crate::tui::theme::PURPLE);
                Ok(())
            }
            CommitAction::CompareCommit => {
                self.app.open_compare_target(source);
                Ok(())
            }
            CommitAction::CherryPickCommit => {
                self.app.open_cherry_pick_target(source);
                Ok(())
            }
            CommitAction::ResetCurrentBranch => self.open_reset_mode_picker(source),
        }
    }

    fn open_reset_mode_picker(&mut self, target: String) -> anyhow::Result<()> {
        let Some(status) = self.app.status.as_ref() else {
            self.app.set_feedback(
                "Reset is unavailable until repository status is loaded.",
                MessageKind::Warning,
            );
            return Ok(());
        };
        if status.branch_name == "HEAD" || status.branch_name.trim().is_empty() {
            self.app.set_feedback(
                "Reset requires a checked-out local branch; detached HEAD is read-only here.",
                MessageKind::Warning,
            );
            return Ok(());
        }
        let branch = status.branch_name.clone();
        if self.app.selected_graph_ref().as_deref() != Some(branch.as_str()) {
            self.app.set_feedback(
                "Reset is available only when the graph displays the current local branch.",
                MessageKind::Warning,
            );
            return Ok(());
        }
        if status_has_unresolved_conflicts(status) {
            self.app.set_feedback(
                "Resolve the current Git conflicts before starting a reset flow.",
                MessageKind::Warning,
            );
            return Ok(());
        }

        let dirty_paths = status
            .files
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let expected_head = self
            .client
            .resolve_commit("HEAD")
            .map_err(anyhow::Error::msg)?;
        self.app.open_reset_mode_picker(ResetReview {
            target,
            branch,
            expected_head,
            dirty_paths,
        });
        Ok(())
    }

    fn start_confirmed_cherry_pick(&mut self) -> anyhow::Result<()> {
        let Some((source, destination)) = self.app.cherry_pick_request() else {
            self.app.set_feedback(
                "Review a local cherry-pick destination before confirming.",
                MessageKind::Warning,
            );
            return Ok(());
        };
        self.app.close_commit_actions();
        self.start_operation_request(OperationRequest::CherryPick {
            source,
            destination,
        })
    }

    fn start_confirmed_reset(&mut self) -> anyhow::Result<()> {
        let Some((target, mode, expected_branch, expected_head)) = self.app.reset_request() else {
            self.app.set_feedback(
                "Review the reset target and mode before confirming.",
                MessageKind::Warning,
            );
            return Ok(());
        };
        self.app.close_commit_actions();
        self.start_operation_request(OperationRequest::Reset {
            target,
            mode,
            expected_branch,
            expected_head,
        })
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

    fn open_cleanup_modal(&mut self) -> anyhow::Result<()> {
        if self.operation_rx.is_some() {
            self.app.set_feedback(
                "Wait for the current operation to finish before cleanup.",
                MessageKind::Warning,
            );
            return Ok(());
        }

        match self.client.merged_local_branches("HEAD", &[], &[]) {
            Ok(candidates) => self.app.open_cleanup_modal(candidates),
            Err(error) => self.app.set_feedback(
                format!("Could not inspect merged local branches: {error}"),
                MessageKind::Error,
            ),
        }
        Ok(())
    }

    fn confirm_cleanup(&mut self) -> anyhow::Result<()> {
        let branches = self.app.selected_cleanup_branches();
        if branches.is_empty() {
            self.app.begin_cleanup_confirmation();
            return Ok(());
        }

        self.app.close_cleanup_modal();
        let request = OperationRequest::CleanupLocalBranches {
            branches,
            base: String::from("HEAD"),
        };
        self.start_operation_request(request)
    }

    fn finish_cleanup(&mut self, report: CleanupReport, refresh_warning: Option<String>) {
        self.app.stop_loading();
        let refresh_failed = refresh_warning.is_some();
        let message = match refresh_warning {
            Some(warning) => format!(
                "Cleanup finished: {} deleted, {} skipped, {} failed. {warning}",
                report.deleted, report.skipped, report.failed
            ),
            None => format!(
                "Cleanup finished: {} deleted, {} skipped, {} failed.",
                report.deleted, report.skipped, report.failed
            ),
        };
        let kind = if report.failed > 0 {
            MessageKind::Error
        } else if report.skipped > 0 || refresh_failed {
            MessageKind::Warning
        } else {
            MessageKind::Success
        };
        self.app.show_cleanup_report(report);
        self.app.set_feedback(message, kind);
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
    use std::{
        fs,
        path::Path,
        process::{Command, Output},
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    use crate::test_support::{
        checkout_branch, commit_all, configure_user, create_branch, current_dir_lock, init_repo,
        write_file, CurrentDirGuard,
    };
    use crate::{
        domain::{BranchInfo, BranchKind, CommitSummary, RepoStatus, StatusEntry},
        git::{GitClient, ResetMode},
        tui::app::CommitActionFlow,
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
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            Intent::MoveCommitPage(1)
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            Intent::MoveCommitPage(-1)
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            Intent::SelectFirstCommit
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            Intent::SelectLastCommit
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Intent::MoveGraphHorizontal(-4)
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Intent::MoveGraphHorizontal(4)
        );
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

    fn controller_with_state() -> TuiController {
        let mut controller = TuiController::new(GitClient::new());
        controller.app_mut().branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "1111111111111111111111111111111111111111".to_string(),
                subject: "main".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "1111111111111111111111111111111111111111".to_string(),
                subject: "main".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        controller.app_mut().selected_branch = Some("main".to_string());
        controller.app_mut().selected_remote_branch = Some("refs/remotes/origin/main".to_string());
        controller.app_mut().status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });
        controller.app_mut().log = vec![CommitSummary {
            hash: "1111111111111111111111111111111111111111".to_string(),
            author: "Marcos".to_string(),
            date: "2026-08-09".to_string(),
            subject: "main".to_string(),
        }];
        controller
    }

    #[test]
    fn intent_routing_covers_global_and_modal_key_paths() {
        let mut controller = controller_with_state();

        for (key, expected) in [
            (KeyCode::Char('q'), Intent::Quit),
            (KeyCode::Esc, Intent::Quit),
            (KeyCode::Char('r'), Intent::Refresh),
            (KeyCode::Char('1'), Intent::SelectView(View::Branches)),
            (KeyCode::Char('2'), Intent::SelectView(View::Log)),
            (KeyCode::Char('h'), Intent::OpenHelp),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }

        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)),
            Intent::None
        );

        controller.app_mut().open_help();
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Intent::Quit
        );
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Intent::None
        );
        controller.app_mut().close_help();

        controller
            .app_mut()
            .open_delete_branch_confirm(DeleteBranchTarget::Local {
                branch: "main".to_string(),
            });
        assert_eq!(
            controller.intent_for_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Intent::None
        );
        controller.app_mut().close_delete_branch_confirm();

        controller
            .app_mut()
            .open_branch_creator_from_source("main".to_string(), crate::tui::theme::SUCCESS);
        for (key, expected) in [
            (KeyCode::Esc, Intent::CancelBranchCreate),
            (KeyCode::Enter, Intent::ConfirmBranchCreate),
            (KeyCode::Backspace, Intent::DeleteBranchName),
            (KeyCode::Char('x'), Intent::TypeBranchName('x')),
            (KeyCode::Down, Intent::None),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }
        controller.app_mut().close_branch_creator();

        controller.app_mut().open_branch_search();
        for (key, expected) in [
            (KeyCode::Esc, Intent::CloseBranchSearch),
            (KeyCode::Enter, Intent::ConfirmBranchSearch),
            (KeyCode::Backspace, Intent::DeleteBranchSearchChar),
            (KeyCode::Char('x'), Intent::TypeBranchSearchChar('x')),
            (KeyCode::Down, Intent::None),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }
        controller.app_mut().close_branch_search();

        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Local);
        controller.app_mut().open_picker();
        for (key, expected) in [
            (KeyCode::Esc, Intent::ClosePicker),
            (KeyCode::Enter, Intent::ConfirmPicker),
            (KeyCode::Down, Intent::MovePicker(1)),
            (KeyCode::Up, Intent::MovePicker(-1)),
            (KeyCode::Char('x'), Intent::None),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }
        controller.app_mut().close_picker();

        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);
        controller.app_mut().open_remote_picker();
        for (key, expected) in [
            (KeyCode::Esc, Intent::CloseRemotePicker),
            (KeyCode::Enter, Intent::ConfirmRemotePicker),
            (KeyCode::Down, Intent::MoveRemotePicker(1)),
            (KeyCode::Up, Intent::MoveRemotePicker(-1)),
            (KeyCode::Char('x'), Intent::None),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }
        controller.app_mut().close_remote_picker();

        controller.app_mut().open_commit_actions();
        for (key, expected) in [
            (KeyCode::Esc, Intent::CloseCommitActions),
            (KeyCode::Enter, Intent::ConfirmCommitAction),
            (KeyCode::Down, Intent::MoveCommitAction(1)),
            (KeyCode::Up, Intent::MoveCommitAction(-1)),
            (KeyCode::Char('x'), Intent::None),
        ] {
            assert_eq!(
                controller.intent_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
                expected
            );
        }
    }

    #[test]
    fn apply_intent_covers_state_only_transitions() {
        let mut controller = controller_with_state();

        assert!(!controller.apply_intent(Intent::None).unwrap());
        assert!(controller.apply_intent(Intent::Quit).unwrap());
        assert!(!controller
            .apply_intent(Intent::SelectView(View::Log))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveCommitSelection(1))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveCommitSelection(-1))
            .unwrap());
        assert!(!controller.apply_intent(Intent::MoveCommitPage(1)).unwrap());
        assert!(!controller.apply_intent(Intent::MoveCommitPage(-1)).unwrap());
        assert!(!controller.apply_intent(Intent::SelectFirstCommit).unwrap());
        assert!(!controller.apply_intent(Intent::SelectLastCommit).unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveGraphHorizontal(4))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveGraphHorizontal(-4))
            .unwrap());
        assert!(!controller.apply_intent(Intent::OpenHelp).unwrap());
        assert!(!controller.apply_intent(Intent::MoveHelpScroll(1)).unwrap());
        assert!(!controller.apply_intent(Intent::CloseHelp).unwrap());

        assert!(!controller
            .apply_intent(Intent::SelectView(View::Branches))
            .unwrap());
        assert!(!controller.apply_intent(Intent::OpenBranchSearch).unwrap());
        assert!(!controller
            .apply_intent(Intent::TypeBranchSearchChar('m'))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::DeleteBranchSearchChar)
            .unwrap());
        controller.app_mut().branch_search_input.clear();
        assert!(!controller
            .apply_intent(Intent::ConfirmBranchSearch)
            .unwrap());
        assert!(!controller.apply_intent(Intent::CloseBranchSearch).unwrap());

        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Local);
        assert!(!controller.apply_intent(Intent::OpenPicker).unwrap());
        assert!(!controller.apply_intent(Intent::MovePicker(1)).unwrap());
        assert!(!controller.apply_intent(Intent::MovePicker(-1)).unwrap());
        assert!(!controller.apply_intent(Intent::ClosePicker).unwrap());

        controller
            .app_mut()
            .set_branch_panel(super::BranchPanel::Remote);
        assert!(!controller.apply_intent(Intent::OpenRemotePicker).unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveRemotePicker(1))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveRemotePicker(-1))
            .unwrap());
        assert!(!controller.apply_intent(Intent::CloseRemotePicker).unwrap());

        assert!(!controller.apply_intent(Intent::OpenCommitActions).unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveCommitAction(1))
            .unwrap());
        assert!(!controller
            .apply_intent(Intent::MoveCommitAction(-1))
            .unwrap());
        assert!(!controller.apply_intent(Intent::CloseCommitActions).unwrap());

        controller
            .app_mut()
            .open_delete_branch_confirm(DeleteBranchTarget::Local {
                branch: "main".to_string(),
            });
        assert!(!controller.apply_intent(Intent::CancelDeleteBranch).unwrap());

        controller
            .app_mut()
            .open_branch_creator_from_source("main".to_string(), crate::tui::theme::SUCCESS);
        assert!(!controller
            .apply_intent(Intent::TypeBranchName('x'))
            .unwrap());
        assert!(!controller.apply_intent(Intent::DeleteBranchName).unwrap());
        assert!(!controller.apply_intent(Intent::CancelBranchCreate).unwrap());
        controller
            .app_mut()
            .open_branch_creator_from_source("main".to_string(), crate::tui::theme::SUCCESS);
        assert!(!controller
            .apply_intent(Intent::ConfirmBranchCreate)
            .unwrap());

        controller.app_mut().start_loading("loading");
        controller.tick();
        controller.app_mut().select_view(View::Log);
        controller.tick();
    }

    #[test]
    fn build_operation_covers_picker_variants_and_invalid_direct_actions() {
        let controller = controller_with_state();

        assert!(matches!(
            controller.build_operation(crate::tui::app::PickerAction::Checkout),
            Ok(crate::tui::operations::OperationRequest::Checkout { .. })
        ));
        assert!(matches!(
            controller.build_operation(crate::tui::app::PickerAction::Switch),
            Ok(crate::tui::operations::OperationRequest::Switch { .. })
        ));
        assert!(matches!(
            controller.build_operation(crate::tui::app::PickerAction::Pull),
            Ok(crate::tui::operations::OperationRequest::Pull {
                remote: Some(_),
                branch: Some(_)
            })
        ));
        assert!(matches!(
            controller.build_operation(crate::tui::app::PickerAction::Push),
            Ok(crate::tui::operations::OperationRequest::Push {
                remote: Some(_),
                branch: Some(_)
            })
        ));
        assert!(controller
            .build_operation(crate::tui::app::PickerAction::CreateBranch)
            .is_err());
        assert!(controller
            .build_operation(crate::tui::app::PickerAction::DeleteBranch)
            .is_err());
    }

    #[test]
    fn poll_operation_covers_empty_success_error_and_disconnect() {
        let mut controller = controller_with_state();

        let (tx, rx) = std::sync::mpsc::channel();
        controller.operation_rx = Some(rx);
        controller.poll_operation().unwrap();
        assert!(controller.operation_rx.is_some());

        tx.send(crate::tui::operations::OperationOutcome::Error(
            "operation failed".to_string(),
        ))
        .unwrap();
        controller.poll_operation().unwrap();
        assert!(controller.operation_rx.is_none());
        assert_eq!(
            controller.app().message_kind,
            crate::tui::app::MessageKind::Error
        );

        let (tx, rx) = std::sync::mpsc::channel();
        controller.operation_rx = Some(rx);
        tx.send(crate::tui::operations::OperationOutcome::Success {
            snapshot: crate::domain::RepoSnapshot {
                status: RepoStatus {
                    branch_name: "success".to_string(),
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                    files: Vec::new(),
                },
                branches: Vec::new(),
                history: crate::domain::BranchHistory::from_graph(Vec::new()),
                selected_branch: None,
            },
            message: "success".to_string(),
        })
        .unwrap();
        controller.poll_operation().unwrap();
        assert_eq!(
            controller.app().status.as_ref().unwrap().branch_name,
            "success"
        );

        let (tx, rx) = std::sync::mpsc::channel::<crate::tui::operations::OperationOutcome>();
        controller.operation_rx = Some(rx);
        drop(tx);
        controller.poll_operation().unwrap();
        assert!(controller.operation_rx.is_none());
        assert_eq!(
            controller.app().message_kind,
            crate::tui::app::MessageKind::Error
        );
    }

    fn commit_action_git(repository: &Path, arguments: &[&str]) -> Output {
        Command::new("git")
            .current_dir(repository)
            .args(arguments)
            .output()
            .unwrap()
    }

    fn commit_action_checked_git(repository: &Path, arguments: &[&str]) -> String {
        let output = commit_action_git(repository, arguments);
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit_action_init_repository() -> (TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        commit_action_checked_git(temp.path(), &["init", "--quiet"]);
        commit_action_checked_git(temp.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
        commit_action_checked_git(temp.path(), &["config", "user.name", "Gitrex Test"]);
        commit_action_checked_git(temp.path(), &["config", "user.email", "gitrex@example.com"]);
        fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
        commit_action_checked_git(temp.path(), &["add", "-A"]);
        commit_action_checked_git(temp.path(), &["commit", "--quiet", "-m", "base"]);
        let base = commit_action_checked_git(temp.path(), &["rev-parse", "HEAD"]);
        (temp, base)
    }

    fn commit_action_linear_controller(
        changed_paths: usize,
        dirty_paths: usize,
    ) -> (TempDir, TuiController, String, String) {
        let (temp, base) = commit_action_init_repository();
        commit_action_checked_git(temp.path(), &["branch", "aaa-destination", base.as_str()]);
        fs::write(temp.path().join("tracked.txt"), "target\n").unwrap();
        for index in 0..changed_paths {
            fs::write(
                temp.path().join(format!("changed-{index:02}.txt")),
                format!("content {index}\n"),
            )
            .unwrap();
        }
        commit_action_checked_git(temp.path(), &["add", "-A"]);
        commit_action_checked_git(temp.path(), &["commit", "--quiet", "-m", "target"]);
        let target = commit_action_checked_git(temp.path(), &["rev-parse", "HEAD"]);

        fs::write(temp.path().join("tracked.txt"), "local edit\n").unwrap();
        for index in 0..dirty_paths {
            fs::write(
                temp.path().join(format!("dirty-{index:02}.txt")),
                format!("untracked {index}\n"),
            )
            .unwrap();
        }

        let mut controller = TuiController::new(GitClient::from_path(temp.path()));
        controller.refresh().unwrap();
        (temp, controller, base, target)
    }

    fn commit_action_press(controller: &mut TuiController, code: KeyCode) {
        controller
            .handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
            .unwrap();
    }

    fn commit_action_open(controller: &mut TuiController, index: usize) {
        controller.app_mut().select_view(View::Log);
        commit_action_press(controller, KeyCode::Enter);
        for _ in 0..index {
            commit_action_press(controller, KeyCode::Char('j'));
        }
        commit_action_press(controller, KeyCode::Enter);
    }

    fn commit_action_render(controller: &mut TuiController, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
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

    fn commit_action_wait(controller: &mut TuiController) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while controller.app().loading.is_some() {
            controller.poll_operation().unwrap();
            assert!(Instant::now() < deadline, "Git operation did not finish");
            thread::sleep(Duration::from_millis(10));
        }
        controller.poll_operation().unwrap();
    }

    fn commit_action_branch_index(controller: &TuiController, name: &str) -> usize {
        controller
            .app()
            .branches
            .iter()
            .filter(|branch| !branch.is_remote())
            .position(|branch| branch.name == name)
            .unwrap()
    }

    #[test]
    fn compare_flow_resolves_target_and_scrolls_long_stat_output() {
        let (temp, mut controller, _, target) = commit_action_linear_controller(32, 0);
        let client = GitClient::from_path(temp.path());
        let status_before = client.status().unwrap();

        commit_action_open(&mut controller, 2);
        for character in "HEAD~1".chars() {
            commit_action_press(&mut controller, KeyCode::Char(character));
        }
        commit_action_press(&mut controller, KeyCode::Enter);
        commit_action_wait(&mut controller);

        assert_eq!(client.resolve_commit("HEAD").unwrap(), target);
        assert_eq!(client.status().unwrap(), status_before);
        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::CompareResult { .. })
        ));
        let first_page = commit_action_render(&mut controller, 70, 16);
        assert!(first_page.contains("changed-00.txt"), "{first_page}");

        for _ in 0..8 {
            commit_action_press(&mut controller, KeyCode::PageDown);
        }
        let last_page = commit_action_render(&mut controller, 70, 16);
        assert!(
            last_page.contains("changed-31.txt"),
            "comparison cannot scroll to its last changed path: {last_page}"
        );
    }

    #[test]
    fn cherry_pick_confirmation_can_be_cancelled_without_switching_branches() {
        let (temp, mut controller, _, source) = commit_action_linear_controller(1, 0);
        let client = GitClient::from_path(temp.path());

        commit_action_open(&mut controller, 3);
        let destination_index = commit_action_branch_index(&controller, "aaa-destination");
        for _ in 0..destination_index {
            commit_action_press(&mut controller, KeyCode::Char('j'));
        }
        commit_action_press(&mut controller, KeyCode::Enter);
        let confirmation = commit_action_render(&mut controller, 90, 24);
        assert!(confirmation.contains("Source commit:"), "{confirmation}");
        assert!(
            confirmation.contains("Destination local branch: aaa-destination"),
            "{confirmation}"
        );
        assert!(confirmation.contains("clean"), "{confirmation}");

        commit_action_press(&mut controller, KeyCode::Esc);

        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::CherryPickTarget { .. })
        ));
        assert_eq!(client.status().unwrap().branch_name, "main");
        assert_eq!(client.resolve_commit("HEAD").unwrap(), source);
        assert!(controller.app().loading.is_none());
    }

    #[test]
    fn cherry_pick_conflict_refreshes_destination_status_and_recovery_guidance() {
        let (temp, base) = commit_action_init_repository();
        commit_action_checked_git(temp.path(), &["branch", "destination", base.as_str()]);

        fs::write(temp.path().join("tracked.txt"), "source edit\n").unwrap();
        commit_action_checked_git(temp.path(), &["add", "tracked.txt"]);
        commit_action_checked_git(temp.path(), &["commit", "--quiet", "-m", "source change"]);

        commit_action_checked_git(temp.path(), &["switch", "--quiet", "destination"]);
        fs::write(temp.path().join("tracked.txt"), "destination edit\n").unwrap();
        commit_action_checked_git(temp.path(), &["add", "tracked.txt"]);
        commit_action_checked_git(
            temp.path(),
            &["commit", "--quiet", "-m", "destination change"],
        );
        commit_action_checked_git(temp.path(), &["switch", "--quiet", "main"]);

        let mut controller = TuiController::new(GitClient::from_path(temp.path()));
        controller.refresh().unwrap();
        commit_action_open(&mut controller, 3);
        let destination_index = commit_action_branch_index(&controller, "destination");
        for _ in 0..destination_index {
            commit_action_press(&mut controller, KeyCode::Char('j'));
        }
        commit_action_press(&mut controller, KeyCode::Enter);
        commit_action_press(&mut controller, KeyCode::Enter);
        commit_action_wait(&mut controller);

        let status = controller.app().status.as_ref().unwrap();
        assert_eq!(status.branch_name, "destination");
        assert!(
            status.files.iter().any(|entry| entry.code.contains('U')),
            "refreshed status does not expose the cherry-pick conflict: {status:?}"
        );
        assert!(controller.app().message.contains("cherry-pick --continue"));
        assert!(controller.app().message.contains("--abort"));
    }

    #[test]
    fn reset_defaults_to_soft_and_escape_cancels_without_mutation() {
        let (temp, mut controller, base, target) = commit_action_linear_controller(1, 0);
        let client = GitClient::from_path(temp.path());
        controller.app_mut().select_view(View::Log);
        commit_action_press(&mut controller, KeyCode::Char('j'));
        commit_action_open(&mut controller, 4);

        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::ResetModePicker {
                selected_index: 0,
                ..
            })
        ));
        let picker = commit_action_render(&mut controller, 90, 24);
        assert!(picker.contains("Soft is the safest default"), "{picker}");
        assert!(picker.contains("▶ Soft"), "{picker}");

        commit_action_press(&mut controller, KeyCode::Enter);
        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::ResetConfirmation {
                mode: ResetMode::Soft,
                ..
            })
        ));
        commit_action_press(&mut controller, KeyCode::Esc);
        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::ResetModePicker { .. })
        ));
        commit_action_press(&mut controller, KeyCode::Esc);

        assert_eq!(client.resolve_commit("HEAD").unwrap(), target);
        assert_eq!(client.resolve_commit("main").unwrap(), target);
        assert_eq!(client.resolve_commit(base.as_str()).unwrap(), base);
        assert!(controller.app().loading.is_none());
    }

    #[test]
    fn hard_reset_requires_y_and_scrolls_every_dirty_path_before_mutating() {
        let (temp, mut controller, base, _) = commit_action_linear_controller(1, 28);
        let client = GitClient::from_path(temp.path());
        controller.app_mut().select_view(View::Log);
        commit_action_press(&mut controller, KeyCode::Char('j'));
        commit_action_open(&mut controller, 4);
        commit_action_press(&mut controller, KeyCode::Char('j'));
        commit_action_press(&mut controller, KeyCode::Char('j'));
        commit_action_press(&mut controller, KeyCode::Enter);
        commit_action_press(&mut controller, KeyCode::Enter);

        assert!(matches!(
            controller.app().commit_action_flow,
            Some(CommitActionFlow::HardResetConfirmation { .. })
        ));
        let first_page = commit_action_render(&mut controller, 70, 14);
        assert!(
            first_page.contains("DESTRUCTIVE HARD RESET"),
            "{first_page}"
        );
        assert!(
            first_page.contains("y = confirm destructive reset"),
            "hard reset confirmation control is hidden: {first_page}"
        );

        commit_action_press(&mut controller, KeyCode::PageDown);
        let first_paths = commit_action_render(&mut controller, 70, 14);
        assert!(
            first_paths.contains("dirty-00.txt"),
            "hard reset review cannot scroll to its first dirty path: {first_paths}"
        );

        for _ in 0..10 {
            commit_action_press(&mut controller, KeyCode::PageDown);
        }
        let last_page = commit_action_render(&mut controller, 70, 14);
        assert!(
            last_page.contains("dirty-27.txt"),
            "hard reset review cannot scroll to the final dirty path: {last_page}"
        );

        commit_action_press(&mut controller, KeyCode::Enter);
        assert!(controller.app().loading.is_none());
        assert_eq!(
            client.resolve_commit("HEAD").unwrap(),
            client.resolve_commit("main").unwrap()
        );

        commit_action_press(&mut controller, KeyCode::Char('y'));
        commit_action_wait(&mut controller);

        assert_eq!(client.resolve_commit("HEAD").unwrap(), base);
        assert_eq!(
            fs::read_to_string(temp.path().join("tracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "base\n"
        );
        assert!(temp.path().join("dirty-27.txt").exists());
    }

    fn commit_action_open_reset(controller: &mut TuiController) {
        controller.app_mut().select_view(View::Log);
        commit_action_open(controller, 4);
    }

    #[test]
    fn reset_is_refused_for_non_current_branch_detached_head_and_conflicts() {
        let cases = [
            (
                "non-current",
                "main",
                Some("aaa-destination"),
                None,
                "current local branch",
            ),
            ("detached", "HEAD", Some("main"), None, "detached HEAD"),
            ("conflicted", "main", Some("main"), Some("UU"), "conflicts"),
        ];

        for (case, branch_name, selected_branch, conflict_code, expected_message) in cases {
            let (temp, mut controller, _, _) = commit_action_linear_controller(0, 0);
            controller.app_mut().status.as_mut().unwrap().branch_name = branch_name.to_string();
            controller.app_mut().selected_branch = selected_branch.map(str::to_string);
            if let Some(code) = conflict_code {
                controller
                    .app_mut()
                    .status
                    .as_mut()
                    .unwrap()
                    .files
                    .push(StatusEntry {
                        code: code.to_string(),
                        path: "conflict.txt".to_string(),
                    });
            }

            commit_action_open_reset(&mut controller);

            assert!(
                matches!(
                    controller.app().commit_action_flow,
                    Some(CommitActionFlow::Menu {
                        selected_index: 4,
                        ..
                    })
                ),
                "{case}: reset unexpectedly opened a mode picker"
            );
            assert!(
                controller.app().message.contains(expected_message),
                "{case}: {}",
                controller.app().message
            );
            assert!(controller.app().loading.is_none());
            drop(temp);
        }
    }
}
