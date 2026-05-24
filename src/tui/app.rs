use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    cli::output,
    domain::{BranchInfo, CommitSummary, RepoStatus},
};

use super::{layout, theme, widgets};

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
    CreateBranch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitAction {
    CheckoutCommit,
    CreateBranchFromCommit,
}

impl CommitAction {
    pub fn label(self) -> &'static str {
        match self {
            CommitAction::CheckoutCommit => "checkout commit",
            CommitAction::CreateBranchFromCommit => "create branch from commit",
        }
    }
}

impl PickerAction {
    pub fn label(self) -> &'static str {
        match self {
            PickerAction::Checkout => "checkout branch",
            PickerAction::Switch => "switch branch",
            PickerAction::Pull => "pull current branch",
            PickerAction::Push => "push current branch",
            PickerAction::CreateBranch => "create branch from source",
        }
    }
}

pub struct App {
    pub(crate) view: View,
    pub(crate) status: Option<RepoStatus>,
    pub(crate) branches: Vec<BranchInfo>,
    pub(crate) log: Vec<CommitSummary>,
    pub(crate) selected_branch: usize,
    pub(crate) selected_commit: usize,
    pub(crate) picker_open: bool,
    pub(crate) picker_index: usize,
    pub(crate) commit_actions_open: bool,
    pub(crate) commit_action_index: usize,
    pub(crate) branch_create_open: bool,
    pub(crate) branch_create_source: Option<String>,
    pub(crate) branch_create_name: String,
    pub(crate) loading: Option<String>,
    pub(crate) message: String,
    pub(crate) message_kind: MessageKind,
}

impl App {
    pub fn new() -> Self {
        Self {
            view: View::Branches,
            status: None,
            branches: Vec::new(),
            log: Vec::new(),
            selected_branch: 0,
            selected_commit: 0,
            picker_open: false,
            picker_index: 0,
            commit_actions_open: false,
            commit_action_index: 0,
            branch_create_open: false,
            branch_create_source: None,
            branch_create_name: String::new(),
            loading: None,
            message: String::from("Press q to quit, r to refresh, Enter for branch actions."),
            message_kind: MessageKind::Info,
        }
    }

    pub fn set_feedback(&mut self, message: impl Into<String>, kind: MessageKind) {
        self.message = message.into();
        self.message_kind = kind;
    }

    pub fn select_view(&mut self, view: View) {
        self.view = view;
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.local_branches().get(self.selected_branch).copied()
    }

    pub fn local_branches(&self) -> Vec<&BranchInfo> {
        self.branches
            .iter()
            .filter(|branch| matches!(branch.kind, crate::domain::BranchKind::Local))
            .collect()
    }

    pub fn picker_actions(&self) -> &'static [PickerAction] {
        const ACTIONS: &[PickerAction] = &[
            PickerAction::Checkout,
            PickerAction::Switch,
            PickerAction::Pull,
            PickerAction::Push,
            PickerAction::CreateBranch,
        ];
        ACTIONS
    }

    pub fn branch_create_is_open(&self) -> bool {
        self.branch_create_open
    }

    pub fn commit_actions_are_open(&self) -> bool {
        self.commit_actions_open
    }

    pub fn sync_target_display(&self) -> Option<String> {
        self.current_sync_target()
            .map(|(remote, branch)| format!("{remote}/{branch}"))
    }

    pub fn move_selection(&mut self, delta: isize) {
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

    pub fn move_commit_selection(&mut self, delta: isize) {
        let commit_count = self.log.len();
        if commit_count == 0 {
            self.selected_commit = 0;
            return;
        }

        let next = if delta.is_negative() {
            self.selected_commit.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected_commit.saturating_add(delta as usize)
        };

        self.selected_commit = next.min(commit_count.saturating_sub(1));
    }

    pub fn open_picker(&mut self) {
        if self.selected_branch().is_none() {
            self.set_feedback("No branch selected.", MessageKind::Warning);
            return;
        }
        self.picker_open = true;
        self.picker_index = 0;
        self.message = "Choose an action for the selected branch.".to_string();
        self.message_kind = MessageKind::Info;
    }

    pub fn close_picker(&mut self) {
        self.picker_open = false;
    }

    pub fn open_commit_actions(&mut self) {
        if self.selected_commit().is_none() {
            self.set_feedback("No commit selected.", MessageKind::Warning);
            return;
        }
        self.commit_actions_open = true;
        self.commit_action_index = 0;
        self.set_feedback("Choose an action for the selected commit.", MessageKind::Info);
    }

    pub fn close_commit_actions(&mut self) {
        self.commit_actions_open = false;
    }

    pub fn open_branch_creator(&mut self) {
        let Some(source) = self.selected_branch().map(|branch| branch.name.clone()) else {
            self.set_feedback("No branch selected.", MessageKind::Warning);
            return;
        };

        self.open_branch_creator_from_source(source);
    }

    pub fn open_branch_creator_from_source(&mut self, source: String) {
        self.branch_create_open = true;
        self.branch_create_source = Some(source);
        self.branch_create_name.clear();
        self.set_feedback(
            "Type a new branch name and press Enter.",
            MessageKind::Info,
        );
    }

    pub fn close_branch_creator(&mut self) {
        self.branch_create_open = false;
        self.branch_create_source = None;
        self.branch_create_name.clear();
    }

    pub fn push_branch_create_char(&mut self, ch: char) {
        if self.branch_create_open && (ch.is_ascii_graphic() || ch == ' ') {
            self.branch_create_name.push(ch);
        }
    }

    pub fn pop_branch_create_char(&mut self) {
        if self.branch_create_open {
            self.branch_create_name.pop();
        }
    }

    pub fn branch_create_request(&self) -> Option<(String, String)> {
        let source = self.branch_create_source.clone()?;
        let branch = self.branch_create_name.trim().to_string();
        if branch.is_empty() {
            return None;
        }
        Some((branch, source))
    }

    pub fn selected_commit(&self) -> Option<&CommitSummary> {
        self.log.get(self.selected_commit)
    }

    pub fn move_picker(&mut self, delta: isize) {
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

    pub fn move_commit_action(&mut self, delta: isize) {
        let count = self.commit_actions().len();
        if count == 0 {
            return;
        }

        let next = if delta.is_negative() {
            self.commit_action_index.saturating_sub(delta.unsigned_abs())
        } else {
            self.commit_action_index.saturating_add(delta as usize)
        };
        self.commit_action_index = next.min(count.saturating_sub(1));
    }

    pub fn commit_actions(&self) -> &'static [CommitAction] {
        const ACTIONS: &[CommitAction] = &[
            CommitAction::CheckoutCommit,
            CommitAction::CreateBranchFromCommit,
        ];
        ACTIONS
    }

    pub fn start_loading(&mut self, label: impl Into<String>) {
        let label = label.into();
        self.loading = Some(label.clone());
        self.set_feedback(format!("{label}..."), MessageKind::Info);
    }

    pub fn stop_loading(&mut self) {
        self.loading = None;
    }

    pub fn current_sync_target(&self) -> Option<(String, String)> {
        let status = self.status.as_ref()?;
        let upstream = status.upstream.as_deref()?;
        let (remote, _) = upstream.split_once('/')?;
        Some((remote.to_string(), status.branch_name.clone()))
    }

    pub fn apply_snapshot(
        &mut self,
        status: RepoStatus,
        branches: Vec<BranchInfo>,
        log: Vec<CommitSummary>,
        selected_branch: usize,
    ) {
        self.status = Some(status);
        self.branches = branches;
        self.log = log;
        self.selected_branch = selected_branch;
        self.selected_commit = self.selected_commit.min(self.log.len().saturating_sub(1));
    }

    pub fn footer_text(&self) -> String {
        match &self.loading {
            Some(loading) => format!("{loading}..."),
            None => self.message.clone(),
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
        frame.render_widget(self.render_graph(), right);
        frame.render_widget(self.render_actions(), actions);
        frame.render_widget(self.render_footer(), footer);
        if self.branch_create_open {
            let popup = self.render_branch_creator();
            let area = layout::centered_rect(60, 34, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(popup, area);
        }
        if self.picker_open {
            let popup = self.render_picker();
            let area = layout::centered_rect(60, 50, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(popup, area);
        }
        if self.commit_actions_open {
            let popup = self.render_commit_actions();
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
                    .title(format!(
                        "Branches ({}/{})",
                        selected.saturating_add(1),
                        self.local_branches().len().max(1)
                    ))
                    .title_style(Style::default().fg(theme::ACCENT))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
    }

    fn render_graph(&self) -> List<'_> {
        let items = output::render_graph_preview(&self.log, self.selected_commit)
            .into_iter()
            .map(ListItem::new)
            .collect::<Vec<_>>();

        List::new(items).block(
            Block::default()
                .title("Git Graph")
                .title_style(Style::default().fg(theme::MUTED))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE_ALT)),
        )
    }

    fn render_actions(&self) -> Paragraph<'_> {
        let copy = if matches!(self.view, View::Log) {
            let commit = self.selected_commit().map(|commit| {
                let short_hash = commit.hash.chars().take(8).collect::<String>();
                format!("{short_hash} {}", commit.subject)
            });
            let commit = commit.as_deref().unwrap_or("no commit selected");
            [
                "Keys:",
                "j/k or arrows = move commits",
                "1/2/3 = change pane",
                "Enter = open commit options",
                "r = refresh",
                "q = quit",
                "",
                "Selected commit:",
                commit,
            ]
            .join("\n")
        } else {
            widgets::actions_copy(
                self.selected_branch().map(|b| b.name.as_str()),
                self.sync_target_display().as_deref(),
            )
        };

        Paragraph::new(copy)
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

    fn render_picker(&self) -> Paragraph<'_> {
        let branch = self
            .selected_branch()
            .map(|branch| branch.name.as_str())
            .unwrap_or("unknown");
        let sync_target = self
            .sync_target_display()
            .unwrap_or_else(|| "no upstream".to_string());
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

        Paragraph::new(format!(
            "Branch: {branch}\nSync target: {sync_target}\n\n{options}\n\nEnter = confirm • Esc = close"
        ))
            .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
            .block(
                Block::default()
                    .title("Branch Actions")
                    .title_style(Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
    }

    fn render_branch_creator(&self) -> Paragraph<'_> {
        let source = self
            .branch_create_source
            .as_deref()
            .unwrap_or("unknown");
        let name = if self.branch_create_name.is_empty() {
            "<type new branch name>"
        } else {
            &self.branch_create_name
        };

        Paragraph::new(format!(
            "Source branch: {source}\nNew branch name: {name}\n\nEnter = create • Esc = cancel • Backspace = delete"
        ))
        .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
        .block(
            Block::default()
                .title("Create Branch")
                .title_style(Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT)),
        )
    }

    fn render_commit_actions(&self) -> Paragraph<'_> {
        let commit = self
            .selected_commit()
            .map(|commit| {
                let short_hash = commit.hash.chars().take(8).collect::<String>();
                format!("{short_hash} {}", commit.subject)
            })
            .unwrap_or_else(|| String::from("unknown commit"));
        let options = self
            .commit_actions()
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let prefix = if index == self.commit_action_index {
                    "▶"
                } else {
                    " "
                };
                format!("{prefix} {}", action.label())
            })
            .collect::<Vec<_>>()
            .join("\n");

        Paragraph::new(format!(
            "Commit: {commit}\n\n{options}\n\nEnter = confirm • Esc = close"
        ))
        .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE))
        .block(
            Block::default()
                .title("Commit Actions")
                .title_style(Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT)),
        )
    }

    fn branch_state(&self) -> ListState {
        let mut state = ListState::default();
        if !self.local_branches().is_empty() {
            state.select(Some(
                self.selected_branch.min(self.local_branches().len().saturating_sub(1)),
            ));
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::{App, CommitAction, MessageKind, PickerAction, View};
    use crate::{
        domain::{BranchInfo, BranchKind, RepoStatus},
    };

    #[test]
    fn picker_action_labels_are_clear() {
        assert_eq!(PickerAction::Checkout.label(), "checkout branch");
        assert_eq!(PickerAction::Push.label(), "push current branch");
    }

    #[test]
    fn feedback_kind_is_settable() {
        let mut app = App::new();
        app.set_feedback("Saved", MessageKind::Success);
        app.select_view(View::Log);
        assert_eq!(app.view, View::Log);
        assert_eq!(app.message, "Saved");
    }

    #[test]
    fn selection_moves_within_local_branches() {
        let mut app = App::new();
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
    fn current_sync_target_uses_upstream_remote_and_local_branch() {
        let mut app = App::new();
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
        let mut app = App::new();
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
        let mut app = App::new();
        app.loading = Some("Pulling changes".to_string());
        app.message = "Pull complete.".to_string();

        assert_eq!(app.footer_text(), "Pulling changes...");
    }

    #[test]
    fn open_branch_creator_uses_selected_source_branch() {
        let mut app = App::new();
        app.branches = vec![BranchInfo {
            name: "main".to_string(),
            current: true,
            upstream: None,
            commit: "abc".to_string(),
            subject: "init".to_string(),
            kind: BranchKind::Local,
        }];

        app.open_branch_creator();

        assert!(app.branch_create_is_open());
        assert_eq!(app.branch_create_source.as_deref(), Some("main"));
        assert!(app.branch_create_name.is_empty());
    }

    #[test]
    fn commit_selection_moves_with_log_entries() {
        let mut app = App::new();
        app.log = vec![
            crate::domain::CommitSummary {
                hash: "abc123".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Initial commit".to_string(),
            },
            crate::domain::CommitSummary {
                hash: "def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Add feature".to_string(),
            },
        ];

        app.move_commit_selection(1);
        assert_eq!(app.selected_commit().unwrap().hash, "def456");

        app.move_commit_selection(1);
        assert_eq!(app.selected_commit().unwrap().hash, "def456");

        app.move_commit_selection(-1);
        assert_eq!(app.selected_commit().unwrap().hash, "abc123");
    }

    #[test]
    fn commit_actions_are_ordered() {
        let app = App::new();
        assert_eq!(app.commit_actions()[0], CommitAction::CheckoutCommit);
        assert_eq!(app.commit_actions()[1], CommitAction::CreateBranchFromCommit);
    }

    #[test]
    fn branch_create_request_uses_trimmed_input_and_source() {
        let mut app = App::new();
        app.branch_create_open = true;
        app.branch_create_source = Some("main".to_string());
        app.branch_create_name = "  feature/login  ".to_string();

        assert_eq!(
            app.branch_create_request(),
            Some(("feature/login".to_string(), "main".to_string()))
        );
    }
}
