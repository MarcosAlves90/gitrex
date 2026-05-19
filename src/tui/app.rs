use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    cli::output,
    domain::{BranchInfo, CommitSummary, RepoStatus},
    git::GitClient,
};

use super::{
    layout,
    operations::{build_snapshot, GitOperationRunner, OperationOutcome, OperationRequest, RepoSnapshot},
    theme, widgets,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Status,
    Branches,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    Checkout,
    Switch,
    Pull,
    Push,
}

impl PickerAction {
    fn label(self) -> &'static str {
        match self {
            PickerAction::Checkout => "checkout branch",
            PickerAction::Switch => "switch branch",
            PickerAction::Pull => "pull current branch",
            PickerAction::Push => "push current branch",
        }
    }
}

pub struct App {
    client: GitClient,
    view: View,
    status: Option<RepoStatus>,
    branches: Vec<BranchInfo>,
    log: Vec<CommitSummary>,
    selected_branch: usize,
    picker_open: bool,
    picker_index: usize,
    loading: Option<String>,
    operation_rx: Option<std::sync::mpsc::Receiver<OperationOutcome>>,
    runner: GitOperationRunner,
    message: String,
    message_kind: MessageKind,
}

impl App {
    pub fn new(client: GitClient) -> Self {
        let runner = GitOperationRunner::new(<GitClient as Clone>::clone(&client));
        Self {
            client,
            view: View::Branches,
            status: None,
            branches: Vec::new(),
            log: Vec::new(),
            selected_branch: 0,
            picker_open: false,
            picker_index: 0,
            loading: None,
            operation_rx: None,
            runner,
            message: String::from("Press q to quit, r to refresh, a for branch actions."),
            message_kind: MessageKind::Info,
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let snapshot = build_snapshot(&self.client).map_err(anyhow::Error::msg)?;
        self.apply_snapshot(snapshot);
        self.message = String::from("Repository refreshed.");
        self.message_kind = MessageKind::Success;
        Ok(())
    }

    pub fn set_feedback(&mut self, message: impl Into<String>, kind: MessageKind) {
        self.message = message.into();
        self.message_kind = kind;
    }

    pub fn select_view(&mut self, view: View) {
        self.view = view;
    }

    pub fn handle_event(&mut self, event: Event) -> anyhow::Result<bool> {
        self.poll_operation()?;
        let intent = match event {
            Event::Key(key) => self.intent_for_key(key),
            _ => Intent::None,
        };
        self.apply_intent(intent)
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.local_branches().get(self.selected_branch).copied()
    }

    fn local_branches(&self) -> Vec<&BranchInfo> {
        self.branches
            .iter()
            .filter(|branch| matches!(branch.kind, crate::domain::BranchKind::Local))
            .collect()
    }

    fn picker_actions(&self) -> &'static [PickerAction] {
        const ACTIONS: &[PickerAction] = &[
            PickerAction::Checkout,
            PickerAction::Switch,
            PickerAction::Pull,
            PickerAction::Push,
        ];
        ACTIONS
    }

    fn intent_for_key(&self, key: KeyEvent) -> Intent {
        if self.picker_open {
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
                self.select_view(view);
                Ok(false)
            }
            Intent::MoveSelection(delta) => {
                self.move_selection(delta);
                Ok(false)
            }
            Intent::OpenPicker => {
                self.open_picker();
                Ok(false)
            }
            Intent::ClosePicker => {
                self.close_picker();
                Ok(false)
            }
            Intent::MovePicker(delta) => {
                self.move_picker(delta);
                Ok(false)
            }
            Intent::ConfirmPicker => {
                self.confirm_picker()?;
                Ok(false)
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let branch_count = self.local_branches().len();
        if branch_count == 0 {
            self.selected_branch = 0;
            return;
        }

        let next = if delta.is_negative() {
            self.selected_branch.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected_branch.saturating_add(delta as usize)
        };

        self.selected_branch = next.min(branch_count.saturating_sub(1));
    }

    fn open_picker(&mut self) {
        if self.selected_branch().is_none() {
            self.set_feedback("No branch selected.", MessageKind::Warning);
            return;
        }
        self.picker_open = true;
        self.picker_index = 0;
        self.message = "Choose an action for the selected branch.".to_string();
        self.message_kind = MessageKind::Info;
    }

    fn close_picker(&mut self) {
        self.picker_open = false;
    }

    fn move_picker(&mut self, delta: isize) {
        let count = self.picker_actions().len();
        if count == 0 {
            return;
        }

        let next = if delta.is_negative() {
            self.picker_index.saturating_sub(delta.unsigned_abs())
        } else {
            self.picker_index.saturating_add(delta as usize)
        };
        self.picker_index = next.min(count.saturating_sub(1));
    }

    fn confirm_picker(&mut self) -> anyhow::Result<()> {
        let action = *self
            .picker_actions()
            .get(self.picker_index)
            .unwrap_or(&PickerAction::Checkout);
        self.close_picker();
        self.start_operation(action)
    }

    fn current_sync_target(&self) -> Option<(String, String)> {
        let status = self.status.as_ref()?;
        let upstream = status.upstream.as_deref()?;
        let (remote, _) = upstream.split_once('/')?;
        Some((remote.to_string(), status.branch_name.clone()))
    }

    fn start_operation(&mut self, action: PickerAction) -> anyhow::Result<()> {
        if self.loading.is_some() {
            return Ok(());
        }

        let operation = self.build_operation(action)?;
        let label = operation.label().to_string();
        self.loading = Some(label.clone());
        self.operation_rx = Some(self.runner.spawn(operation));
        self.set_feedback(format!("{label}..."), MessageKind::Info);

        Ok(())
    }

    fn build_operation(&self, action: PickerAction) -> anyhow::Result<OperationRequest> {
        match action {
            PickerAction::Checkout => {
                let branch = self
                    .selected_branch()
                    .map(|branch| branch.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
                Ok(OperationRequest::Checkout { branch })
            }
            PickerAction::Switch => {
                let branch = self
                    .selected_branch()
                    .map(|branch| branch.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("No branch selected."))?;
                Ok(OperationRequest::Switch { branch })
            }
            PickerAction::Pull => {
                let sync = self.current_sync_target();
                let (remote, branch) = sync
                    .map(|(remote, branch)| (Some(remote), Some(branch)))
                    .unwrap_or((None, None));
                Ok(OperationRequest::Pull { remote, branch })
            }
            PickerAction::Push => {
                let sync = self.current_sync_target();
                let (remote, branch) = sync
                    .map(|(remote, branch)| (Some(remote), Some(branch)))
                    .unwrap_or((None, None));
                Ok(OperationRequest::Push { remote, branch })
            }
        }
    }

    fn apply_snapshot(&mut self, snapshot: RepoSnapshot) {
        self.status = snapshot.status;
        self.branches = snapshot.branches;
        self.log = snapshot.log;
        self.selected_branch = snapshot.selected_branch;
    }

    fn finish_operation(&mut self, outcome: OperationOutcome) -> anyhow::Result<()> {
        self.loading = None;
        self.operation_rx = None;

        match outcome {
            OperationOutcome::Success { snapshot, message } => {
                self.apply_snapshot(snapshot);
                self.set_feedback(message, MessageKind::Success);
            }
            OperationOutcome::Error(message) => {
                self.set_feedback(message, MessageKind::Error);
            }
        }

        Ok(())
    }

    pub fn poll_operation(&mut self) -> anyhow::Result<()> {
        let Some(rx) = self.operation_rx.as_ref() else {
            return Ok(());
        };

        match rx.try_recv() {
            Ok(outcome) => self.finish_operation(outcome),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.loading = None;
                self.operation_rx = None;
                self.set_feedback("Operation aborted unexpectedly.", MessageKind::Error);
                Ok(())
            }
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let [header, body, actions, footer] = layout::dashboard(frame.area());
        let [left, right] = layout::body(body);
        let [status_area, branches_area] = layout::left_column(left);

        frame.render_widget(self.render_header(), header);
        frame.render_widget(self.render_status(), status_area);
        let mut branch_state = self.branch_state();
        frame.render_stateful_widget(self.render_branches(), branches_area, &mut branch_state);
        frame.render_widget(self.render_log(), right);
        frame.render_widget(self.render_actions(), actions);
        frame.render_widget(self.render_footer(), footer);
        if self.picker_open {
            let popup = self.render_picker();
            let area = layout::centered_rect(60, 50, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(popup, area);
        }
    }

    fn render_header(&self) -> Paragraph<'_> {
        let text = match &self.status {
            Some(status) => format!(
                "{}  •  {} file(s) changed  •  {}",
                status.branch_name,
                status.files.len(),
                widgets::mode_label(self.view)
            ),
            None => format!("No repository loaded  •  {}", widgets::mode_label(self.view)),
        };

        Paragraph::new(text)
            .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
            .block(
                Block::default()
                    .title("gitrex")
                    .title_style(Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
    }

    fn render_status(&self) -> Paragraph<'_> {
        let text = self
            .status
            .as_ref()
            .map(output::render_status_summary)
            .unwrap_or_else(|| "no repository loaded".to_string());

        Paragraph::new(text)
            .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
            .block(
                Block::default()
                    .title("Status")
                    .title_style(Style::default().fg(theme::WARNING))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::WARNING)),
            )
    }

    fn render_branches(&self) -> List<'_> {
        let selected = self.selected_branch.min(self.local_branches().len().saturating_sub(1));
        let items = self
            .local_branches()
            .into_iter()
            .map(|branch| {
                let marker = if branch.current { "●" } else { " " };
                let text = format!("{marker} {}", branch.name);
                let style = if branch.current {
                    Style::default()
                        .fg(theme::SUCCESS)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT)
                };

                ListItem::new(text).style(style)
            })
            .collect::<Vec<_>>();

        List::new(items)
            .highlight_style(
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ")
            .block(
                Block::default()
                    .title(format!("Branches ({}/{})", selected.saturating_add(1), self.local_branches().len().max(1)))
                    .title_style(Style::default().fg(theme::ACCENT))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
    }

    fn render_log(&self) -> List<'_> {
        let items = output::render_log_preview(&self.log)
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>();

        List::new(items).block(
            Block::default()
                .title("Recent Log")
                .title_style(Style::default().fg(theme::MUTED))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE_ALT)),
        )
    }

    fn render_actions(&self) -> Paragraph<'_> {
        Paragraph::new(widgets::actions_copy(self.selected_branch().map(|b| b.name.as_str())))
            .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE_ALT))
            .block(
                Block::default()
                    .title("Actions")
                    .title_style(Style::default().fg(theme::SUCCESS))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SUCCESS)),
            )
    }

    fn render_footer(&self) -> Paragraph<'_> {
        Paragraph::new(self.footer_text())
            .style(
                Style::default()
                    .fg(match self.message_kind {
                        MessageKind::Info => theme::TEXT,
                        MessageKind::Success => theme::SUCCESS,
                        MessageKind::Warning => theme::WARNING,
                        MessageKind::Error => theme::ERROR,
                    })
                    .bg(theme::SURFACE),
            )
            .block(
                Block::default()
                    .title("Message")
                    .title_style(Style::default().fg(theme::MUTED))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE_ALT)),
            )
    }

    fn footer_text(&self) -> String {
        match &self.loading {
            Some(loading) => format!("{loading}..."),
            None => self.message.clone(),
        }
    }

    fn render_picker(&self) -> Paragraph<'_> {
        let branch = self.selected_branch().map(|branch| branch.name.as_str()).unwrap_or("unknown");
        let options = self
            .picker_actions()
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let prefix = if index == self.picker_index { "▶" } else { " " };
                format!("{prefix} {}", action.label())
            })
            .collect::<Vec<_>>()
            .join("\n");

        Paragraph::new(format!("Branch: {branch}\n\n{options}\n\nEnter = confirm • Esc = close"))
            .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
            .block(
                Block::default()
                    .title("Branch Actions")
                    .title_style(Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
    }

    fn branch_state(&self) -> ListState {
        let mut state = ListState::default();
        if !self.local_branches().is_empty() {
            state.select(Some(self.selected_branch.min(self.local_branches().len().saturating_sub(1))));
        }
        state
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
}

#[cfg(test)]
mod tests {
    use super::{App, Intent, MessageKind, PickerAction, View};
    use crate::{
        domain::{BranchInfo, BranchKind, RepoStatus},
        git::GitClient,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn key_map_opens_picker_for_branch_actions() {
        let app = App::new(GitClient::new());
        assert_eq!(
            app.intent_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::OpenPicker
        );
        assert_eq!(
            app.intent_for_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Intent::Quit
        );
    }

    #[test]
    fn picker_action_labels_are_clear() {
        assert_eq!(PickerAction::Checkout.label(), "checkout branch");
        assert_eq!(PickerAction::Push.label(), "push current branch");
    }

    #[test]
    fn feedback_kind_is_settable() {
        let mut app = App::new(GitClient::new());
        app.set_feedback("Saved", MessageKind::Success);
        app.select_view(View::Log);
        assert_eq!(app.view, View::Log);
        assert_eq!(app.message, "Saved");
    }

    #[test]
    fn selection_moves_within_local_branches() {
        let mut app = App::new(GitClient::new());
        app.branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "def".to_string(),
                subject: "feature".to_string(),
                kind: BranchKind::Local,
            },
        ];

        app.move_selection(1);
        assert_eq!(app.selected_branch().unwrap().name, "feature/login");

        app.move_selection(1);
        assert_eq!(app.selected_branch().unwrap().name, "feature/login");

        app.move_selection(-1);
        assert_eq!(app.selected_branch().unwrap().name, "main");
    }

    #[test]
    fn picker_opens_and_closes() {
        let mut app = App::new(GitClient::new());
        app.branches = vec![BranchInfo {
            name: "main".to_string(),
            current: true,
            upstream: None,
            commit: "abc".to_string(),
            subject: "init".to_string(),
            kind: BranchKind::Local,
        }];

        app.open_picker();
        assert!(app.picker_open);
        app.close_picker();
        assert!(!app.picker_open);
    }

    #[test]
    fn picker_state_changes_with_escape_and_confirm() {
        let mut app = App::new(GitClient::new());
        app.branches = vec![BranchInfo {
            name: "main".to_string(),
            current: true,
            upstream: None,
            commit: "abc".to_string(),
            subject: "init".to_string(),
            kind: BranchKind::Local,
        }];

        app.open_picker();
        assert!(app.picker_open);
        assert!(!app.handle_event(crossterm::event::Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))).unwrap());
        assert!(!app.picker_open);
    }

    #[test]
    fn current_sync_target_uses_upstream_remote_and_local_branch() {
        let mut app = App::new(GitClient::new());
        app.status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });

        assert_eq!(
            app.current_sync_target(),
            Some(("origin".to_string(), "main".to_string()))
        );
    }

    #[test]
    fn current_sync_target_returns_none_without_upstream() {
        let mut app = App::new(GitClient::new());
        app.status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: None,
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });

        assert_eq!(app.current_sync_target(), None);
    }

    #[test]
    fn loading_text_overrides_footer_message() {
        let mut app = App::new(GitClient::new());
        app.loading = Some("Pulling changes".to_string());
        app.message = "Pull complete.".to_string();

        assert_eq!(app.footer_text(), "Pulling changes...");
    }
}
