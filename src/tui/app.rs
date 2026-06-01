use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    cli::output,
    domain::{
        build_branch_catalog, BranchHistory, BranchInfo, CommitSummary, GraphLine, RepoStatus,
    },
};

use super::{layout, theme, widgets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Status,
    Branches,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPanel {
    Local,
    Remote,
}

impl BranchPanel {
    pub fn label(self) -> &'static str {
        match self {
            BranchPanel::Local => "local",
            BranchPanel::Remote => "remote",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            BranchPanel::Local => BranchPanel::Remote,
            BranchPanel::Remote => BranchPanel::Local,
        }
    }
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
pub enum RemoteBranchAction {
    CreateLocalBranch,
    CheckoutDetached,
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

impl RemoteBranchAction {
    pub fn label(self) -> &'static str {
        match self {
            RemoteBranchAction::CreateLocalBranch => "create local branch",
            RemoteBranchAction::CheckoutDetached => "checkout detached HEAD",
        }
    }
}

pub struct App {
    pub(crate) view: View,
    pub(crate) status: Option<RepoStatus>,
    pub(crate) branches: Vec<BranchInfo>,
    pub(crate) log: Vec<CommitSummary>,
    pub(crate) graph: Vec<GraphLine>,
    pub(crate) selected_branch: Option<String>,
    pub(crate) selected_remote_branch: Option<String>,
    pub(crate) branch_panel: BranchPanel,
    pub(crate) selected_commit: usize,
    pub(crate) picker_open: bool,
    pub(crate) picker_index: usize,
    pub(crate) remote_picker_open: bool,
    pub(crate) remote_picker_index: usize,
    pub(crate) commit_actions_open: bool,
    pub(crate) commit_action_index: usize,
    pub(crate) graph_scroll_offset: usize,
    pub(crate) branch_filter: String,
    pub(crate) branch_search_open: bool,
    pub(crate) branch_search_input: String,
    pub(crate) branch_create_open: bool,
    pub(crate) branch_create_source: Option<String>,
    pub(crate) branch_create_accent: ratatui::style::Color,
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
            graph: Vec::new(),
            selected_branch: None,
            selected_remote_branch: None,
            branch_panel: BranchPanel::Local,
            selected_commit: 0,
            picker_open: false,
            picker_index: 0,
            remote_picker_open: false,
            remote_picker_index: 0,
            commit_actions_open: false,
            commit_action_index: 0,
            graph_scroll_offset: 0,
            branch_filter: String::new(),
            branch_search_open: false,
            branch_search_input: String::new(),
            branch_create_open: false,
            branch_create_source: None,
            branch_create_accent: theme::ACCENT,
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
        if matches!(self.view, View::Branches) {
            self.ensure_active_branch_selection_visible();
        }
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        let selected = self.selected_branch.as_deref()?;
        self.branches.iter().find(|branch| {
            matches!(branch.kind, crate::domain::BranchKind::Local) && branch.name == selected
        })
    }

    pub fn selected_remote_branch(&self) -> Option<&BranchInfo> {
        let selected = self.selected_remote_branch.as_deref()?;
        self.branches.iter().find(|branch| {
            matches!(branch.kind, crate::domain::BranchKind::Remote)
                && branch.full_ref() == selected
        })
    }

    pub fn branch_panel(&self) -> BranchPanel {
        self.branch_panel
    }

    pub fn set_branch_panel(&mut self, panel: BranchPanel) {
        self.branch_panel = panel;
        self.ensure_active_branch_selection_visible();
    }

    pub fn toggle_branch_panel(&mut self) {
        self.set_branch_panel(self.branch_panel.toggled());
    }

    pub fn local_branches(&self) -> Vec<&BranchInfo> {
        self.filtered_local_branches()
    }

    pub fn remote_branches(&self) -> Vec<&BranchInfo> {
        self.filtered_remote_branches()
    }

    pub fn open_branch_search(&mut self) {
        self.branch_search_open = true;
        self.branch_search_input = self.branch_filter.clone();
        self.set_feedback(
            "Type a branch filter and press Enter to apply.",
            MessageKind::Info,
        );
    }

    pub fn branch_search_is_open(&self) -> bool {
        self.branch_search_open
    }

    pub fn push_branch_search_char(&mut self, ch: char) {
        if self.branch_search_open && (ch.is_ascii_graphic() || ch == ' ') {
            self.branch_search_input.push(ch);
        }
    }

    pub fn pop_branch_search_char(&mut self) {
        if self.branch_search_open {
            self.branch_search_input.pop();
        }
    }

    pub fn confirm_branch_search(&mut self) -> bool {
        if !self.branch_search_open {
            return false;
        }

        let previous = self.selected_graph_ref();
        self.branch_search_open = false;
        self.branch_filter = self.branch_search_input.trim().to_string();
        self.ensure_selected_local_branch_visible();
        self.ensure_selected_remote_branch_visible();
        previous != self.selected_graph_ref()
    }

    pub fn close_branch_search(&mut self) {
        self.branch_search_open = false;
        self.branch_search_input.clear();
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

    pub fn remote_picker_is_open(&self) -> bool {
        self.remote_picker_open
    }

    pub fn commit_actions_are_open(&self) -> bool {
        self.commit_actions_open
    }

    pub fn sync_target_display(&self) -> Option<String> {
        self.current_sync_target()
            .map(|(remote, branch)| format!("{remote}/{branch}"))
    }

    pub fn selected_graph_ref(&self) -> Option<String> {
        match self.branch_panel {
            BranchPanel::Local => self
                .selected_branch()
                .map(|branch| branch.name.clone())
                .or_else(|| {
                    self.status
                        .as_ref()
                        .map(|status| status.branch_name.clone())
                }),
            BranchPanel::Remote => self
                .selected_remote_branch()
                .map(|branch| branch.full_ref())
                .or_else(|| {
                    self.selected_branch()
                        .map(|branch| branch.name.clone())
                        .or_else(|| {
                            self.status
                                .as_ref()
                                .map(|status| status.branch_name.clone())
                        })
                }),
        }
    }

    pub fn selected_graph_label(&self) -> Option<String> {
        match self.branch_panel {
            BranchPanel::Local => self
                .selected_branch()
                .map(|branch| branch.name.clone())
                .or_else(|| {
                    self.status
                        .as_ref()
                        .map(|status| status.branch_name.clone())
                }),
            BranchPanel::Remote => self
                .selected_remote_branch()
                .map(|branch| branch.display_name())
                .or_else(|| {
                    self.selected_branch()
                        .map(|branch| branch.name.clone())
                        .or_else(|| {
                            self.status
                                .as_ref()
                                .map(|status| status.branch_name.clone())
                        })
                }),
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.branch_panel {
            BranchPanel::Local => self.move_local_selection(delta),
            BranchPanel::Remote => self.move_remote_selection(delta),
        }
    }

    fn move_local_selection(&mut self, delta: isize) {
        let branches = self.filtered_local_branches();
        let branch_count = branches.len();
        if branch_count == 0 {
            self.selected_branch = None;
            return;
        }

        let current_index = self
            .selected_branch
            .as_deref()
            .and_then(|selected| branches.iter().position(|branch| branch.name == selected))
            .unwrap_or(0);
        let next = if delta.is_negative() {
            current_index.saturating_sub(delta.unsigned_abs())
        } else {
            current_index.saturating_add(delta as usize)
        };
        let selected = branches[next.min(branch_count.saturating_sub(1))]
            .name
            .clone();
        self.selected_branch = Some(selected);
    }

    fn move_remote_selection(&mut self, delta: isize) {
        let branches = self.filtered_remote_branches();
        let branch_count = branches.len();
        if branch_count == 0 {
            self.selected_remote_branch = None;
            return;
        }

        let current_index = self
            .selected_remote_branch
            .as_deref()
            .and_then(|selected| {
                branches
                    .iter()
                    .position(|branch| branch.full_ref() == selected)
            })
            .unwrap_or(0);
        let next = if delta.is_negative() {
            current_index.saturating_sub(delta.unsigned_abs())
        } else {
            current_index.saturating_add(delta as usize)
        };
        let selected = branches[next.min(branch_count.saturating_sub(1))].full_ref();
        self.selected_remote_branch = Some(selected);
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

        if next != self.selected_commit {
            self.graph_scroll_offset = 0;
        }
        self.selected_commit = next.min(commit_count.saturating_sub(1));
    }

    pub fn advance_graph_scroll(&mut self) {
        if self.log.is_empty() {
            self.graph_scroll_offset = 0;
            return;
        }
        self.graph_scroll_offset = self.graph_scroll_offset.wrapping_add(1);
    }

    pub fn open_picker(&mut self) {
        self.ensure_active_branch_selection_visible();
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

    pub fn open_remote_picker(&mut self) {
        self.ensure_active_branch_selection_visible();
        if self.selected_remote_branch().is_none() {
            self.set_feedback("No remote branch selected.", MessageKind::Warning);
            return;
        }
        self.remote_picker_open = true;
        self.remote_picker_index = 0;
        self.message = "Choose an action for the selected remote branch.".to_string();
        self.message_kind = MessageKind::Info;
    }

    pub fn close_remote_picker(&mut self) {
        self.remote_picker_open = false;
    }

    pub fn open_commit_actions(&mut self) {
        if self.selected_commit().is_none() {
            self.set_feedback("No commit selected.", MessageKind::Warning);
            return;
        }
        self.commit_actions_open = true;
        self.commit_action_index = 0;
        self.set_feedback(
            "Choose an action for the selected commit.",
            MessageKind::Info,
        );
    }

    pub fn close_commit_actions(&mut self) {
        self.commit_actions_open = false;
    }

    pub fn open_branch_creator(&mut self) {
        self.ensure_active_branch_selection_visible();
        let Some(source) = self.selected_branch().map(|branch| branch.name.clone()) else {
            self.set_feedback("No branch selected.", MessageKind::Warning);
            return;
        };

        self.open_branch_creator_from_source(source, theme::ACCENT);
    }

    pub fn open_branch_creator_from_source(
        &mut self,
        source: String,
        accent: ratatui::style::Color,
    ) {
        self.branch_create_open = true;
        self.branch_create_source = Some(source);
        self.branch_create_accent = accent;
        self.branch_create_name.clear();
        self.set_feedback("Type a new branch name and press Enter.", MessageKind::Info);
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

    fn branch_filter_matches(&self, branch: &BranchInfo) -> bool {
        let query = self.branch_filter.trim();
        if query.is_empty() {
            return true;
        }

        let query = query.to_ascii_lowercase();
        let display_name = branch.display_name().to_ascii_lowercase();
        [
            branch.name.to_ascii_lowercase(),
            branch.commit.to_ascii_lowercase(),
            branch.subject.to_ascii_lowercase(),
            branch
                .upstream
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            display_name,
        ]
        .iter()
        .any(|haystack| haystack.contains(&query))
    }

    fn filtered_remote_branches(&self) -> Vec<&BranchInfo> {
        let mut branches = self
            .branches
            .iter()
            .filter(|branch| matches!(branch.kind, crate::domain::BranchKind::Remote))
            .filter(|branch| self.branch_filter_matches(branch))
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| {
            left.remote_name()
                .cmp(&right.remote_name())
                .then_with(|| left.branch_short_name().cmp(right.branch_short_name()))
                .then_with(|| right.commit.cmp(&left.commit))
        });
        branches
    }

    fn filtered_local_branches(&self) -> Vec<&BranchInfo> {
        let mut branches = self
            .branches
            .iter()
            .filter(|branch| matches!(branch.kind, crate::domain::BranchKind::Local))
            .filter(|branch| self.branch_filter_matches(branch))
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| {
            right
                .current
                .cmp(&left.current)
                .then_with(|| left.name.cmp(&right.name))
        });
        branches
    }

    fn ensure_selected_local_branch_visible(&mut self) -> bool {
        let local_branches = self.filtered_local_branches();
        if local_branches.is_empty() {
            let changed = self.selected_branch.is_some();
            self.selected_branch = None;
            return changed;
        }

        let selected_visible = self.selected_branch.as_deref().and_then(|selected| {
            local_branches
                .iter()
                .position(|branch| branch.name == selected)
        });
        if selected_visible.is_some() {
            return false;
        }

        let selected = local_branches[0].name.clone();
        let changed = self.selected_branch.as_deref() != Some(selected.as_str());
        self.selected_branch = Some(selected);
        changed
    }

    fn ensure_selected_remote_branch_visible(&mut self) -> bool {
        let remote_branches = self.filtered_remote_branches();
        if remote_branches.is_empty() {
            let changed = self.selected_remote_branch.is_some();
            self.selected_remote_branch = None;
            return changed;
        }

        let selected_visible = self.selected_remote_branch.as_deref().and_then(|selected| {
            remote_branches
                .iter()
                .position(|branch| branch.full_ref() == selected)
        });
        if selected_visible.is_some() {
            return false;
        }

        let selected = remote_branches[0].full_ref();
        let changed = self.selected_remote_branch.as_deref() != Some(selected.as_str());
        self.selected_remote_branch = Some(selected);
        changed
    }

    fn ensure_active_branch_selection_visible(&mut self) {
        match self.branch_panel {
            BranchPanel::Local => {
                let _ = self.ensure_selected_local_branch_visible();
            }
            BranchPanel::Remote => {
                let _ = self.ensure_selected_remote_branch_visible();
            }
        }
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
            self.commit_action_index
                .saturating_sub(delta.unsigned_abs())
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

    pub fn remote_branch_actions(&self) -> &'static [RemoteBranchAction] {
        const ACTIONS: &[RemoteBranchAction] = &[
            RemoteBranchAction::CreateLocalBranch,
            RemoteBranchAction::CheckoutDetached,
        ];
        ACTIONS
    }

    pub fn move_remote_picker(&mut self, delta: isize) {
        let count = self.remote_branch_actions().len();
        if count == 0 {
            return;
        }

        let next = if delta.is_negative() {
            self.remote_picker_index
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.remote_picker_index.saturating_add(delta as usize)
        };
        self.remote_picker_index = next.min(count.saturating_sub(1));
    }

    pub fn selected_remote_action(&self) -> Option<RemoteBranchAction> {
        self.remote_branch_actions()
            .get(self.remote_picker_index)
            .copied()
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
        history: BranchHistory,
        selected_branch: Option<String>,
    ) {
        self.status = Some(status);
        self.branches = branches;
        self.log = history.commits;
        self.graph = history.graph;
        self.selected_branch = selected_branch;
        self.selected_commit = self.selected_commit.min(self.log.len().saturating_sub(1));
        let _ = self.ensure_selected_local_branch_visible();
        let _ = self.ensure_selected_remote_branch_visible();
    }

    pub fn apply_graph_history(&mut self, history: BranchHistory) {
        self.log = history.commits;
        self.graph = history.graph;
        self.selected_commit = 0;
        self.graph_scroll_offset = 0;
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
        let [search_area, remote_area, local_area] = layout::branch_sections(branches_area);

        frame.render_widget(self.render_header(), header);
        frame.render_widget(self.render_status(), status_area);
        frame.render_widget(self.render_branch_search(), search_area);
        let mut remote_state = self.remote_branch_state();
        frame.render_stateful_widget(
            self.render_remote_branches(),
            remote_area,
            &mut remote_state,
        );
        let mut branch_state = self.branch_state();
        frame.render_stateful_widget(self.render_local_branches(), local_area, &mut branch_state);
        let mut graph_state = self.graph_state();
        frame.render_stateful_widget(self.render_graph(right.width), right, &mut graph_state);
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
        if self.remote_picker_open {
            let popup = self.render_remote_picker();
            let area = layout::centered_rect(60, 42, frame.area());
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
        let mut spans = Vec::new();

        match &self.status {
            Some(status) => {
                spans.push(Span::styled(
                    status.branch_name.clone(),
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw("  •  "));
                spans.push(Span::styled(
                    format!("{} file(s) changed", status.files.len()),
                    Style::default().fg(theme::MUTED),
                ));
            }
            None => {
                spans.push(Span::styled(
                    "No repository loaded",
                    Style::default()
                        .fg(theme::MUTED)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        spans.push(Span::raw("  •  "));

        match self.view {
            View::Branches => {
                let local_active = matches!(self.branch_panel, BranchPanel::Local);
                let remote_active = matches!(self.branch_panel, BranchPanel::Remote);
                spans.push(Span::styled(
                    "branches / ",
                    Style::default().fg(theme::MUTED),
                ));
                spans.push(Span::styled(
                    "local",
                    Style::default()
                        .fg(if local_active {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        })
                        .add_modifier(if local_active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
                spans.push(Span::raw("  •  "));
                spans.push(Span::styled(
                    "branches / ",
                    Style::default().fg(theme::MUTED),
                ));
                spans.push(Span::styled(
                    "remote",
                    Style::default()
                        .fg(if remote_active {
                            theme::PURPLE
                        } else {
                            theme::MUTED
                        })
                        .add_modifier(if remote_active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
            }
            _ => {
                spans.push(Span::styled(
                    widgets::mode_label(self.view),
                    Style::default().fg(theme::MUTED),
                ));
            }
        }

        Paragraph::new(Line::from(spans))
            .style(theme::panel_surface_style(true))
            .block(
                Block::default()
                    .title("gitrex")
                    .title_style(theme::panel_title_style(true, theme::ACCENT))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(true, theme::ACCENT)),
            )
    }

    fn render_status(&self) -> Paragraph<'_> {
        let active = matches!(self.view, View::Status);
        let accent = theme::WARNING;
        let text = self
            .status
            .as_ref()
            .map(output::render_status_summary)
            .unwrap_or_else(|| "no repository loaded".to_string());

        Paragraph::new(text)
            .style(theme::panel_surface_style(active))
            .block(
                Block::default()
                    .title("Status")
                    .title_style(theme::panel_title_style(active, accent))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, accent)),
            )
    }

    fn render_branch_search(&self) -> Paragraph<'_> {
        let active = self.branch_search_open;
        let accent = theme::ACCENT;
        let text = if self.branch_search_open {
            format!(
                "Search branches: {}_\nEnter = apply • Esc = cancel",
                self.branch_search_input
            )
        } else if self.branch_filter.trim().is_empty() {
            String::from("Search branches with / to filter both local and remote refs.")
        } else {
            format!(
                "Search filter: {}\nPress / to edit or clear the filter.",
                self.branch_filter
            )
        };

        Paragraph::new(text)
            .style(theme::panel_surface_style(active))
            .block(
                Block::default()
                    .title("Branch Search")
                    .title_style(theme::panel_title_style(active, accent))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, accent)),
            )
    }

    fn render_remote_branches(&self) -> List<'_> {
        let active = matches!(self.branch_panel, BranchPanel::Remote);
        let title_color = theme::PURPLE;
        let border_color = theme::PURPLE;
        let item_style = theme::panel_item_style(active);
        let branches = self.remote_branches();
        let total = branches.len();
        let selected = self
            .selected_remote_branch
            .as_deref()
            .and_then(|selected| {
                branches
                    .iter()
                    .position(|branch| branch.full_ref() == selected)
            })
            .unwrap_or(0);
        let items = branches
            .into_iter()
            .map(|branch| {
                let marker = if branch.current { "●" } else { " " };
                let text = format!(
                    "{marker} {}/{}",
                    branch.remote_name().unwrap_or("remote"),
                    branch.branch_short_name()
                );
                let style = if branch.current {
                    Style::default()
                        .fg(if active { theme::SUCCESS } else { theme::MUTED })
                        .add_modifier(Modifier::BOLD)
                } else {
                    item_style
                };

                ListItem::new(text).style(style)
            })
            .collect::<Vec<_>>();
        let items = if items.is_empty() {
            vec![ListItem::new(
                "No remote branches match the current filter.",
            )]
        } else {
            items
        };

        List::new(items)
            .highlight_style(theme::panel_highlight_style(active, theme::PURPLE))
            .highlight_symbol("▶ ")
            .block(
                Block::default()
                    .title(format!(
                        "Remote branches ({}/{})",
                        selected.saturating_add(1),
                        total.max(1)
                    ))
                    .title_style(theme::panel_title_style(active, title_color))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, border_color)),
            )
    }

    fn render_local_branches(&self) -> List<'_> {
        let active = matches!(self.branch_panel, BranchPanel::Local);
        let title_color = theme::ACCENT;
        let border_color = theme::ACCENT;
        let item_style = theme::panel_item_style(active);
        let local_branches = self.local_branches();
        let catalog = build_branch_catalog(&self.branches);
        let total = local_branches.len();
        let selected = self
            .selected_branch
            .as_deref()
            .and_then(|selected| {
                local_branches
                    .iter()
                    .position(|branch| branch.name == selected)
            })
            .unwrap_or(0);
        let items = local_branches
            .into_iter()
            .map(|branch| {
                let marker = if branch.current { "●" } else { " " };
                let synced = catalog
                    .locals
                    .iter()
                    .find(|entry| entry.branch.name == branch.name)
                    .map(|entry| entry.synced_remotes.clone())
                    .unwrap_or_default();
                let sync_label = if synced.is_empty() {
                    String::from(" [local-only]")
                } else {
                    format!(" [synced: {}]", synced.join(", "))
                };
                let text = format!("{marker} {}{sync_label}", branch.name);
                let style = if branch.current {
                    Style::default()
                        .fg(if active { theme::SUCCESS } else { theme::MUTED })
                        .add_modifier(Modifier::BOLD)
                } else {
                    item_style
                };

                ListItem::new(text).style(style)
            })
            .collect::<Vec<_>>();
        let items = if items.is_empty() {
            vec![ListItem::new("No local branches match the current filter.")]
        } else {
            items
        };

        List::new(items)
            .highlight_style(theme::panel_highlight_style(active, theme::ACCENT))
            .highlight_symbol("▶ ")
            .block(
                Block::default()
                    .title(format!(
                        "Local branches ({}/{})",
                        selected.saturating_add(1),
                        total.max(1)
                    ))
                    .title_style(theme::panel_title_style(active, title_color))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, border_color)),
            )
    }

    fn render_graph(&self, width: u16) -> List<'_> {
        let active = matches!(self.view, View::Log);
        let title_color = theme::PURPLE;
        let border_color = theme::PURPLE;
        let item_style = theme::panel_highlight_style(active, theme::PURPLE);
        let items = output::render_graph_rows(
            &self.graph,
            self.selected_commit,
            self.graph_scroll_offset,
            width,
            active,
        )
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();
        let title = output::render_graph_title(self.selected_graph_label().as_deref());

        List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .title_style(theme::panel_title_style(active, title_color))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, border_color)),
            )
            .highlight_style(item_style)
            .highlight_symbol("▶ ")
    }

    fn graph_state(&self) -> ListState {
        let mut state = ListState::default();
        if !self.graph.is_empty() {
            state.select(self.selected_commit_row_index());
        }
        state
    }

    fn selected_commit_row_index(&self) -> Option<usize> {
        let mut commit_index = 0usize;
        for (row_index, row) in self.graph.iter().enumerate() {
            if matches!(row, GraphLine::Commit { .. }) {
                if commit_index == self.selected_commit {
                    return Some(row_index);
                }
                commit_index = commit_index.saturating_add(1);
            }
        }
        None
    }

    fn render_actions(&self) -> Paragraph<'_> {
        let active = matches!(self.view, View::Log);
        let title_color = theme::SUCCESS;
        let border_color = theme::SUCCESS;
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
            let filter = if self.branch_filter.trim().is_empty() {
                "none".to_string()
            } else {
                self.branch_filter.clone()
            };
            format!(
                "{}\n\nSearch filter:\n{}",
                widgets::actions_copy(
                    self.selected_graph_label().as_deref(),
                    self.sync_target_display().as_deref(),
                    self.branch_panel,
                ),
                filter
            )
        };

        Paragraph::new(copy)
            .style(theme::panel_surface_style(active))
            .block(
                Block::default()
                    .title("Actions")
                    .title_style(theme::panel_title_style(active, title_color))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(active, border_color)),
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
                    }),
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
        let active = matches!(self.branch_panel, BranchPanel::Local);
        let border_color = theme::ACCENT;
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
                let prefix = if index == self.picker_index {
                    "▶"
                } else {
                    " "
                };
                format!("{prefix} {}", action.label())
            })
            .collect::<Vec<_>>()
            .join("\n");

        Paragraph::new(format!(
            "Branch: {branch}\nSync target: {sync_target}\n\n{options}\n\nEnter = confirm • Esc = close"
        ))
            .style(theme::panel_surface_style(active))
        .block(
            Block::default()
                .title("Branch Actions")
                .title_style(theme::panel_title_style(active, border_color))
                .borders(Borders::ALL)
                .border_style(theme::panel_border_style(active, border_color)),
        )
    }

    fn render_remote_picker(&self) -> Paragraph<'_> {
        let active = matches!(self.branch_panel, BranchPanel::Remote);
        let border_color = theme::PURPLE;
        let branch = self
            .selected_remote_branch()
            .map(|branch| branch.display_name())
            .unwrap_or_else(|| String::from("unknown"));
        let options = self
            .remote_branch_actions()
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let prefix = if index == self.remote_picker_index {
                    "▶"
                } else {
                    " "
                };
                format!("{prefix} {}", action.label())
            })
            .collect::<Vec<_>>()
            .join("\n");

        Paragraph::new(format!(
            "Remote branch: {branch}\n\n{options}\n\nEnter = confirm • Esc = close"
        ))
            .style(theme::panel_surface_style(active))
        .block(
            Block::default()
                .title("Remote Branch Actions")
                .title_style(theme::panel_title_style(active, border_color))
                .borders(Borders::ALL)
                .border_style(theme::panel_border_style(active, border_color)),
        )
    }

    fn render_branch_creator(&self) -> Paragraph<'_> {
        let source = self.branch_create_source.as_deref().unwrap_or("unknown");
        let name = if self.branch_create_name.is_empty() {
            "<type new branch name>"
        } else {
            &self.branch_create_name
        };

        Paragraph::new(format!(
            "Source branch: {source}\nNew branch name: {name}\n\nEnter = create • Esc = cancel • Backspace = delete"
        ))
            .style(theme::panel_surface_style(true))
        .block(
            Block::default()
                .title("Create Branch")
                .title_style(theme::panel_title_style(true, self.branch_create_accent))
                .borders(Borders::ALL)
                .border_style(theme::panel_border_style(true, self.branch_create_accent)),
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
            .style(theme::panel_surface_style(true))
        .block(
            Block::default()
                .title("Commit Actions")
                .title_style(theme::panel_title_style(true, theme::PURPLE))
                .borders(Borders::ALL)
                .border_style(theme::panel_border_style(true, theme::PURPLE)),
        )
    }

    fn branch_state(&self) -> ListState {
        let mut state = ListState::default();
        let branches = self.local_branches();
        if !branches.is_empty() {
            let selected = self
                .selected_branch
                .as_deref()
                .and_then(|selected| branches.iter().position(|branch| branch.name == selected))
                .unwrap_or(0);
            state.select(Some(selected.min(branches.len().saturating_sub(1))));
        }
        state
    }

    fn remote_branch_state(&self) -> ListState {
        let mut state = ListState::default();
        let branches = self.remote_branches();
        if !branches.is_empty() {
            let selected = self
                .selected_remote_branch
                .as_deref()
                .and_then(|selected| {
                    branches
                        .iter()
                        .position(|branch| branch.full_ref() == selected)
                })
                .unwrap_or(0);
            state.select(Some(selected.min(branches.len().saturating_sub(1))));
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::{theme, App, BranchPanel, CommitAction, MessageKind, PickerAction, View};
    use crate::domain::{BranchInfo, BranchKind, RepoStatus};

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
    fn remote_selection_moves_with_active_remote_panel() {
        let mut app = App::new();
        app.branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "def".to_string(),
                subject: "feature".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "upstream/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        app.set_branch_panel(BranchPanel::Remote);

        app.move_selection(1);
        assert_eq!(
            app.selected_remote_branch().unwrap().full_ref(),
            "refs/remotes/upstream/main"
        );

        app.move_selection(-1);
        assert_eq!(
            app.selected_remote_branch().unwrap().full_ref(),
            "refs/remotes/origin/feature/login"
        );
    }

    #[test]
    fn switching_branch_panels_materializes_visible_selection() {
        let mut app = App::new();
        app.branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
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
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "upstream/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
        ];

        app.set_branch_panel(BranchPanel::Remote);
        assert_eq!(
            app.selected_remote_branch().unwrap().full_ref(),
            "refs/remotes/origin/main"
        );
        app.open_remote_picker();
        assert!(app.remote_picker_is_open());
        app.close_remote_picker();

        app.set_branch_panel(BranchPanel::Local);
        assert_eq!(app.selected_branch().unwrap().name, "main");
        app.open_picker();
        assert!(app.picker_open);
    }

    #[test]
    fn branch_search_keeps_selection_on_visible_local_branches() {
        let mut app = App::new();
        app.branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
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
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "abc".to_string(),
                subject: "init".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        app.move_selection(1);
        assert_eq!(app.selected_branch().unwrap().name, "feature/login");

        app.open_branch_search();
        app.branch_search_input = "main".to_string();
        assert!(app.confirm_branch_search());
        assert_eq!(app.selected_branch().unwrap().name, "main");
        assert_eq!(app.local_branches().len(), 1);
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
        app.selected_branch = Some("main".to_string());

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
    fn commit_selection_reset_scroll_offset() {
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
        app.graph_scroll_offset = 42;

        app.move_commit_selection(1);

        assert_eq!(app.graph_scroll_offset, 0);
    }

    #[test]
    fn commit_actions_are_ordered() {
        let app = App::new();
        assert_eq!(app.commit_actions()[0], CommitAction::CheckoutCommit);
        assert_eq!(
            app.commit_actions()[1],
            CommitAction::CreateBranchFromCommit
        );
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

    #[test]
    fn branch_create_request_preserves_remote_source_ref() {
        let mut app = App::new();
        app.branch_create_open = true;
        app.branch_create_source = Some("refs/remotes/origin/main".to_string());
        app.branch_create_name = "release".to_string();

        assert_eq!(
            app.branch_create_request(),
            Some((
                "release".to_string(),
                "refs/remotes/origin/main".to_string()
            ))
        );
    }

    #[test]
    fn branch_creator_tracks_the_origin_accent() {
        let mut app = App::new();

        app.open_branch_creator_from_source("refs/remotes/origin/main".to_string(), theme::PURPLE);

        assert!(app.branch_create_is_open());
        assert_eq!(app.branch_create_accent, theme::PURPLE);
    }
}
