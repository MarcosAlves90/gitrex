use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

use crate::{
    cli::output,
    domain::{
        build_branch_catalog, BranchHistory, BranchInfo, CommitSummary, GraphLine, RepoSnapshot,
        RepoStatus,
    },
};

use super::{branching, layout, theme, widgets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Status,
    Branches,
    Log,
    Help,
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
    DeleteBranch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBranchAction {
    CreateLocalBranch,
    CheckoutDetached,
    DeleteRemoteBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteBranchTarget {
    Local { branch: String },
    Remote { remote: String, branch: String },
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
            PickerAction::DeleteBranch => "delete branch",
        }
    }
}

impl RemoteBranchAction {
    pub fn label(self) -> &'static str {
        match self {
            RemoteBranchAction::CreateLocalBranch => "create local branch",
            RemoteBranchAction::CheckoutDetached => "checkout detached HEAD",
            RemoteBranchAction::DeleteRemoteBranch => "delete remote branch",
        }
    }
}

impl DeleteBranchTarget {
    pub fn display_name(&self) -> String {
        match self {
            DeleteBranchTarget::Local { branch } => branch.clone(),
            DeleteBranchTarget::Remote { remote, branch } => format!("{remote}/{branch}"),
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
    pub(crate) delete_branch_confirm_open: bool,
    pub(crate) delete_branch_target: Option<DeleteBranchTarget>,
    pub(crate) graph_scroll_offset: usize,
    pub(crate) branch_filter: String,
    pub(crate) branch_search_open: bool,
    pub(crate) branch_search_input: String,
    pub(crate) branch_create_open: bool,
    pub(crate) branch_create_source: Option<String>,
    pub(crate) branch_create_accent: ratatui::style::Color,
    pub(crate) branch_create_name: String,
    pub(crate) help_return_view: Option<View>,
    pub(crate) help_scroll_offset: usize,
    pub(crate) loading: Option<String>,
    pub(crate) loading_frame: usize,
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
            delete_branch_confirm_open: false,
            delete_branch_target: None,
            graph_scroll_offset: 0,
            branch_filter: String::new(),
            branch_search_open: false,
            branch_search_input: String::new(),
            branch_create_open: false,
            branch_create_source: None,
            branch_create_accent: theme::ACCENT,
            branch_create_name: String::new(),
            help_return_view: None,
            help_scroll_offset: 0,
            loading: None,
            loading_frame: 0,
            message: String::from(
                "Press h for help, q to quit, r to refresh, Enter for branch actions.",
            ),
            message_kind: MessageKind::Info,
        }
    }

    pub fn set_feedback(&mut self, message: impl Into<String>, kind: MessageKind) {
        self.message = message.into();
        self.message_kind = kind;
    }

    pub fn select_view(&mut self, view: View) {
        let was_help_open = self.help_is_open();
        self.view = view;
        if was_help_open {
            self.help_return_view = Some(view);
        }
        if matches!(self.view, View::Branches) {
            self.ensure_active_branch_selection_visible();
        }
    }

    pub fn help_is_open(&self) -> bool {
        matches!(self.view, View::Help)
    }

    pub fn open_help(&mut self) {
        if self.help_is_open() {
            return;
        }
        self.help_return_view = Some(self.view);
        self.help_scroll_offset = 0;
        self.view = View::Help;
        self.set_feedback("Press h or Esc to close help.", MessageKind::Info);
    }

    pub fn close_help(&mut self) {
        let previous = self.help_return_view.take().unwrap_or(View::Branches);
        self.help_scroll_offset = 0;
        self.view = previous;
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

    pub fn local_branch_panel_is_active(&self) -> bool {
        matches!(self.view, View::Branches) && matches!(self.branch_panel, BranchPanel::Local)
    }

    pub fn remote_branch_panel_is_active(&self) -> bool {
        matches!(self.view, View::Branches) && matches!(self.branch_panel, BranchPanel::Remote)
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
            PickerAction::DeleteBranch,
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

    pub fn delete_branch_confirm_is_open(&self) -> bool {
        self.delete_branch_confirm_open
    }

    pub fn delete_branch_target(&self) -> Option<DeleteBranchTarget> {
        self.delete_branch_target.clone()
    }

    pub fn sync_target_display(&self) -> Option<String> {
        self.current_sync_target()
            .map(|(remote, branch)| format!("{remote}/{branch}"))
    }

    pub fn selected_graph_ref(&self) -> Option<String> {
        branching::selected_graph_ref(
            self.branch_panel,
            self.selected_branch(),
            self.selected_remote_branch(),
            self.status
                .as_ref()
                .map(|status| status.branch_name.as_str()),
        )
    }

    pub fn selected_graph_label(&self) -> Option<String> {
        branching::selected_graph_label(
            self.branch_panel,
            self.selected_branch(),
            self.selected_remote_branch(),
            self.status
                .as_ref()
                .map(|status| status.branch_name.as_str()),
        )
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

    pub fn move_help_scroll(&mut self, delta: isize) {
        if delta.is_negative() {
            self.help_scroll_offset = self.help_scroll_offset.saturating_sub(delta.unsigned_abs());
        } else {
            self.help_scroll_offset = self.help_scroll_offset.saturating_add(delta as usize);
        }
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

    pub fn open_delete_branch_confirm(&mut self, target: DeleteBranchTarget) {
        self.delete_branch_confirm_open = true;
        self.delete_branch_target = Some(target);
        self.set_feedback(
            "Delete confirmation is extra dangerous. Enter = confirm, Esc = cancel.",
            MessageKind::Warning,
        );
    }

    pub fn close_delete_branch_confirm(&mut self) {
        self.delete_branch_confirm_open = false;
        self.delete_branch_target = None;
    }

    pub fn open_branch_creator(&mut self) {
        self.ensure_active_branch_selection_visible();
        let Some(source) = self.selected_branch().map(|branch| branch.name.clone()) else {
            self.set_feedback("No branch selected.", MessageKind::Warning);
            return;
        };

        self.open_branch_creator_from_source(source, theme::SUCCESS);
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

    fn filtered_remote_branches(&self) -> Vec<&BranchInfo> {
        branching::filtered_remote_branches(self.branches.as_slice(), self.branch_filter.as_str())
    }

    fn filtered_local_branches(&self) -> Vec<&BranchInfo> {
        branching::filtered_local_branches(self.branches.as_slice(), self.branch_filter.as_str())
    }

    fn ensure_selected_local_branch_visible(&mut self) -> bool {
        branching::ensure_selected_local_branch_visible(
            self.branches.as_slice(),
            self.branch_filter.as_str(),
            &mut self.selected_branch,
        )
    }

    fn ensure_selected_remote_branch_visible(&mut self) -> bool {
        branching::ensure_selected_remote_branch_visible(
            self.branches.as_slice(),
            self.branch_filter.as_str(),
            &mut self.selected_remote_branch,
        )
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
            RemoteBranchAction::DeleteRemoteBranch,
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
        self.loading_frame = 0;
        self.set_feedback(format!("{label}..."), MessageKind::Info);
    }

    pub fn stop_loading(&mut self) {
        self.loading = None;
        self.loading_frame = 0;
    }

    pub fn advance_loading_frame(&mut self) {
        if self.loading.is_some() && self.status.is_none() {
            self.loading_frame = self.loading_frame.wrapping_add(1);
        }
    }

    pub fn current_sync_target(&self) -> Option<(String, String)> {
        branching::current_sync_target(self.status.as_ref())
    }

    pub fn apply_snapshot(&mut self, snapshot: RepoSnapshot) {
        self.status = Some(snapshot.status);
        self.branches = snapshot.branches;
        self.log = snapshot.history.commits;
        self.graph = snapshot.history.graph;
        self.selected_branch = snapshot.selected_branch;
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

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        if self.loading.is_some() && self.status.is_none() {
            self.render_loading_splash(frame);
            return;
        }

        let [header, body, footer] = layout::dashboard(frame.area());
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
        if self.delete_branch_confirm_is_open() {
            let popup = self.render_delete_branch_confirm();
            let area = layout::centered_rect(66, 38, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(popup, area);
        }
        if self.help_is_open() {
            self.render_help(frame);
        }
    }

    fn render_loading_splash(&self, frame: &mut Frame<'_>) {
        let [art_area, text_area] = layout::loading_splash(frame.area());
        frame.render_widget(Clear, frame.area());
        frame.render_widget(
            Paragraph::new(widgets::loading_splash_lines())
                .style(Style::default().fg(theme::WARNING))
                .alignment(Alignment::Center),
            art_area,
        );
        frame.render_widget(
            Paragraph::new(widgets::loading_splash_text(self.loading_frame))
                .style(
                    Style::default()
                        .fg(theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center),
            text_area.inner(Margin {
                vertical: 0,
                horizontal: 1,
            }),
        );
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
                            theme::SUCCESS
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
                            theme::TEAL
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

        spans.push(Span::raw("  •  "));
        spans.push(Span::styled(
            "h = help",
            Style::default()
                .fg(theme::WARNING)
                .add_modifier(Modifier::BOLD),
        ));

        Paragraph::new(Line::from(spans))
            .style(theme::panel_surface_style(true))
            .block(
                Block::default()
                    .title("gitrex")
                    .title_style(theme::panel_title_style(true, theme::ACTIONS))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(true, theme::ACTIONS)),
            )
    }

    fn render_status(&self) -> Paragraph<'_> {
        let active = matches!(self.view, View::Status);
        let accent = theme::STATUS;
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
        let active = self.remote_branch_panel_is_active();
        let title_color = theme::TEAL;
        let border_color = theme::TEAL;
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
            .highlight_style(theme::panel_highlight_style(active, theme::TEAL))
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
        let active = self.local_branch_panel_is_active();
        let title_color = theme::SUCCESS;
        let border_color = theme::SUCCESS;
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
                let relation = catalog
                    .locals
                    .iter()
                    .find(|entry| entry.branch.name == branch.name)
                    .map(|entry| (&entry.synced_remotes, &entry.differing_remotes));
                let sync_label = match relation {
                    None => String::from(" [local-only]"),
                    Some((synced, differing)) if synced.is_empty() && differing.is_empty() => {
                        String::from(" [local-only]")
                    }
                    Some((synced, differing)) if differing.is_empty() => {
                        format!(" [synced: {}]", synced.join(", "))
                    }
                    Some((synced, differing)) if synced.is_empty() => {
                        format!(" [differs: {}]", differing.join(", "))
                    }
                    Some((synced, differing)) => format!(
                        " [synced: {}; differs: {}]",
                        synced.join(", "),
                        differing.join(", ")
                    ),
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
            .highlight_style(theme::panel_highlight_style(active, theme::SUCCESS))
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

    fn render_footer(&self) -> Paragraph<'_> {
        Paragraph::new(self.footer_text())
            .style(Style::default().fg(match self.message_kind {
                MessageKind::Info => theme::TEXT,
                MessageKind::Success => theme::SUCCESS,
                MessageKind::Warning => theme::WARNING,
                MessageKind::Error => theme::ERROR,
            }))
            .block(
                Block::default()
                    .title("Message")
                    .title_style(theme::panel_title_style(true, theme::ACTIONS))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(true, theme::ACTIONS)),
            )
    }

    fn render_picker(&self) -> Paragraph<'_> {
        let active = matches!(self.branch_panel, BranchPanel::Local);
        let border_color = theme::SUCCESS;
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
        let border_color = theme::TEAL;
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

    fn render_delete_branch_confirm(&self) -> Paragraph<'_> {
        let (title, body) = match self.delete_branch_target.as_ref() {
            Some(target @ DeleteBranchTarget::Local { .. }) => (
                "Delete Local Branch",
                format!(
                    "Local branch: {}\n\nThis removes the branch ref from the repository.\nEnter = delete branch • Esc = cancel",
                    target.display_name()
                ),
            ),
            Some(target @ DeleteBranchTarget::Remote { .. }) => (
                "Delete Remote Branch",
                format!(
                    "Remote branch: {}\n\nThis deletes the branch on the remote and refreshes remote refs.\nEnter = delete branch • Esc = cancel",
                    target.display_name()
                ),
            ),
            None => (
                "Delete Branch",
                String::from("No branch selected.\n\nEnter = close • Esc = close"),
            ),
        };

        Paragraph::new(body)
            .style(theme::panel_surface_style(true))
            .block(
                Block::default()
                    .title(title)
                    .title_style(theme::panel_title_style(true, theme::ERROR))
                    .borders(Borders::ALL)
                    .border_style(theme::panel_border_style(true, theme::ERROR)),
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

    fn render_help(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let [header, body, footer] = layout::dashboard(area);
        let active_panel = self.branch_panel;
        let help_lines = widgets::help_lines(
            self.selected_graph_label().as_deref(),
            self.sync_target_display().as_deref(),
            active_panel,
        );
        let inner = body.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let areas = Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        let text_area = areas[0];
        let scrollbar_area = areas[1];
        let content_length = widgets::wrapped_height(&help_lines, text_area.width as usize);
        let viewport_height = text_area.height as usize;
        let max_scroll = content_length.saturating_sub(viewport_height);
        let scroll_offset = self.help_scroll_offset.min(max_scroll);
        self.help_scroll_offset = scroll_offset;
        let scrollbar_content_length = if content_length > viewport_height {
            max_scroll.saturating_add(1)
        } else {
            0
        };
        let mut scrollbar_state = ScrollbarState::new(scrollbar_content_length)
            .position(scroll_offset)
            .viewport_content_length(viewport_height);

        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("")
                .style(theme::panel_surface_style(true))
                .block(
                    Block::default()
                        .title("gitrex help")
                        .title_style(theme::panel_title_style(true, theme::WARNING))
                        .borders(Borders::ALL)
                        .border_style(theme::panel_border_style(true, theme::WARNING)),
                ),
            header,
        );
        frame.render_widget(
            Block::default()
                .title("Shortcuts")
                .title_style(theme::panel_title_style(true, theme::TEXT))
                .borders(Borders::ALL)
                .border_style(theme::panel_border_style(true, theme::TEXT)),
            body,
        );
        frame.render_widget(
            Paragraph::new(help_lines)
                .style(theme::panel_surface_style(true))
                .scroll((scroll_offset as u16, 0))
                .wrap(Wrap { trim: false }),
            text_area,
        );
        if scrollbar_content_length > 0 {
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("↑"))
                    .end_symbol(Some("↓")),
                scrollbar_area,
                &mut scrollbar_state,
            );
        }
        frame.render_widget(
            Paragraph::new(self.footer_text())
                .style(Style::default().fg(match self.message_kind {
                    MessageKind::Info => theme::TEXT,
                    MessageKind::Success => theme::SUCCESS,
                    MessageKind::Warning => theme::WARNING,
                    MessageKind::Error => theme::ERROR,
                }))
                .block(
                    Block::default()
                        .title("Message")
                        .title_style(theme::panel_title_style(true, theme::ACTIONS))
                        .borders(Borders::ALL)
                        .border_style(theme::panel_border_style(true, theme::ACTIONS)),
                ),
            footer,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        theme, App, BranchPanel, CommitAction, DeleteBranchTarget, MessageKind, PickerAction,
        RemoteBranchAction, View,
    };
    use crate::domain::{BranchInfo, BranchKind, RepoStatus};

    #[test]
    fn picker_action_labels_are_clear() {
        assert_eq!(PickerAction::Checkout.label(), "checkout branch");
        assert_eq!(PickerAction::Push.label(), "push current branch");
        assert_eq!(PickerAction::DeleteBranch.label(), "delete branch");
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
    fn help_view_opens_and_closes_back_to_previous_view() {
        let mut app = App::new();
        app.select_view(View::Log);

        app.open_help();
        assert_eq!(app.help_scroll_offset, 0);
        assert!(app.help_is_open());
        assert_eq!(app.view, View::Help);

        app.move_help_scroll(3);
        assert_eq!(app.help_scroll_offset, 3);

        app.close_help();
        assert_eq!(app.help_scroll_offset, 0);
        assert_eq!(app.view, View::Log);

        app.open_help();
        app.select_view(View::Status);
        app.close_help();
        assert_eq!(app.view, View::Status);
    }

    #[test]
    fn help_scroll_moves_without_underflow() {
        let mut app = App::new();
        app.open_help();

        app.move_help_scroll(4);
        app.move_help_scroll(-2);
        app.move_help_scroll(-20);

        assert_eq!(app.help_scroll_offset, 0);
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
    fn branch_panels_only_show_active_colors_in_branches_view() {
        let mut app = App::new();

        app.select_view(View::Log);
        app.set_branch_panel(BranchPanel::Local);
        assert!(!app.local_branch_panel_is_active());
        assert!(!app.remote_branch_panel_is_active());

        app.select_view(View::Branches);
        assert!(app.local_branch_panel_is_active());
        assert!(!app.remote_branch_panel_is_active());

        app.set_branch_panel(BranchPanel::Remote);
        assert!(!app.local_branch_panel_is_active());
        assert!(app.remote_branch_panel_is_active());
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
    fn actions_accent_is_shared_with_gitrex_and_message() {
        assert_eq!(theme::ACTIONS, theme::WARNING);
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
        assert_eq!(app.branch_create_accent, theme::SUCCESS);
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
    fn picker_actions_include_delete_branch_last() {
        let app = App::new();
        assert_eq!(
            app.picker_actions().last().copied(),
            Some(PickerAction::DeleteBranch)
        );
    }

    #[test]
    fn remote_actions_include_delete_branch_last() {
        let app = App::new();
        assert_eq!(
            app.remote_branch_actions().last().copied(),
            Some(RemoteBranchAction::DeleteRemoteBranch)
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

    #[test]
    fn delete_branch_confirmation_tracks_target() {
        let mut app = App::new();

        app.open_delete_branch_confirm(DeleteBranchTarget::Local {
            branch: "feature/login".to_string(),
        });

        assert!(app.delete_branch_confirm_is_open());
        assert_eq!(
            app.delete_branch_target(),
            Some(DeleteBranchTarget::Local {
                branch: "feature/login".to_string()
            })
        );
        assert!(app.footer_text().contains("extra dangerous"));

        app.close_delete_branch_confirm();
        assert!(!app.delete_branch_confirm_is_open());
        assert_eq!(app.delete_branch_target(), None);
    }

    fn populated_app() -> App {
        let mut app = App::new();
        app.status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 1,
            behind: 2,
            files: vec![crate::domain::StatusEntry {
                code: " M".to_string(),
                path: "README.md".to_string(),
            }],
        });
        app.branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "1111111111111111111111111111111111111111".to_string(),
                subject: "main work".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "2222222222222222222222222222222222222222".to_string(),
                subject: "feature work".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "1111111111111111111111111111111111111111".to_string(),
                subject: "main work".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "upstream/main".to_string(),
                current: false,
                upstream: None,
                commit: "3333333333333333333333333333333333333333".to_string(),
                subject: "different main".to_string(),
                kind: BranchKind::Remote,
            },
        ];
        let first = crate::domain::CommitSummary {
            hash: "1111111111111111111111111111111111111111".to_string(),
            author: "Marcos".to_string(),
            date: "2026-08-09".to_string(),
            subject: "A deliberately long commit subject that exercises scrolling".to_string(),
        };
        let second = crate::domain::CommitSummary {
            hash: "2222222222222222222222222222222222222222".to_string(),
            author: "Marcos".to_string(),
            date: "2026-08-08".to_string(),
            subject: "previous commit".to_string(),
        };
        app.log = vec![first.clone(), second.clone()];
        app.graph = vec![
            crate::domain::GraphLine::Commit {
                graph: "*".to_string(),
                summary: first,
            },
            crate::domain::GraphLine::Connector {
                graph: "|".to_string(),
            },
            crate::domain::GraphLine::Commit {
                graph: "*".to_string(),
                summary: second,
            },
        ];
        app.selected_branch = Some("main".to_string());
        app.selected_remote_branch = Some("refs/remotes/origin/main".to_string());
        app
    }

    fn render_text(app: &mut App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
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
    fn render_exercises_dashboard_views_overlays_help_and_loading() {
        let mut app = populated_app();

        app.select_view(View::Branches);
        let dashboard = render_text(&mut app, 120, 40);
        assert!(dashboard.contains("Remote branches"));
        assert!(dashboard.contains("Local branches"));
        assert!(dashboard.contains("Git Graph"));
        assert!(dashboard.contains("main"));

        app.select_view(View::Status);
        assert!(render_text(&mut app, 120, 40).contains("Status"));
        app.select_view(View::Log);
        app.advance_graph_scroll();
        assert!(render_text(&mut app, 120, 40).contains("Git Graph"));

        app.select_view(View::Branches);
        app.set_branch_panel(BranchPanel::Local);
        app.open_picker();
        assert!(render_text(&mut app, 120, 40).contains("Branch Actions"));
        app.move_picker(99);
        app.move_picker(-99);
        app.close_picker();

        app.set_branch_panel(BranchPanel::Remote);
        app.open_remote_picker();
        assert!(render_text(&mut app, 120, 40).contains("Remote Branch Actions"));
        app.move_remote_picker(99);
        app.move_remote_picker(-99);
        app.close_remote_picker();

        app.open_branch_creator_from_source("refs/remotes/origin/main".to_string(), theme::TEAL);
        for ch in "feature/render".chars() {
            app.push_branch_create_char(ch);
        }
        assert!(render_text(&mut app, 120, 40).contains("Create Branch"));
        app.pop_branch_create_char();
        app.close_branch_creator();

        app.open_commit_actions();
        app.move_commit_action(1);
        assert!(render_text(&mut app, 120, 40).contains("Commit Actions"));
        app.move_commit_action(-1);
        app.close_commit_actions();

        app.open_delete_branch_confirm(DeleteBranchTarget::Local {
            branch: "feature/login".to_string(),
        });
        assert!(render_text(&mut app, 120, 40).contains("Delete Local Branch"));
        app.close_delete_branch_confirm();

        app.open_delete_branch_confirm(DeleteBranchTarget::Remote {
            remote: "origin".to_string(),
            branch: "feature/login".to_string(),
        });
        assert!(render_text(&mut app, 120, 40).contains("Delete Remote Branch"));
        app.close_delete_branch_confirm();

        app.delete_branch_confirm_open = true;
        app.delete_branch_target = None;
        assert!(render_text(&mut app, 120, 40).contains("Delete Branch"));
        app.close_delete_branch_confirm();

        app.open_branch_search();
        for ch in "main".chars() {
            app.push_branch_search_char(ch);
        }
        assert!(render_text(&mut app, 120, 40).contains("Search branches"));
        app.pop_branch_search_char();
        app.close_branch_search();

        app.open_help();
        app.move_help_scroll(500);
        let help = render_text(&mut app, 72, 18);
        assert!(help.contains("gitrex help"));
        assert!(app.help_scroll_offset > 0);
        let wide_help = render_text(&mut app, 180, 60);
        assert!(wide_help.contains("Shortcuts"));
        app.close_help();

        for kind in [
            MessageKind::Info,
            MessageKind::Success,
            MessageKind::Warning,
            MessageKind::Error,
        ] {
            app.set_feedback("message", kind);
            assert!(render_text(&mut app, 120, 40).contains("message"));
        }

        let mut empty = App::new();
        let empty_dashboard = render_text(&mut empty, 120, 40);
        assert!(empty_dashboard.contains("No repository loaded"));
        assert!(empty_dashboard.contains("No local branches"));
        assert!(empty_dashboard.contains("No remote branches"));

        let mut loading = App::new();
        loading.start_loading("Loading repository");
        loading.advance_loading_frame();
        let splash = render_text(&mut loading, 90, 28);
        assert!(!splash.trim().is_empty());
        assert_eq!(loading.loading_frame, 1);
        loading.stop_loading();
    }

    #[test]
    fn state_edge_cases_cover_empty_filters_snapshots_and_input_guards() {
        let mut app = App::new();

        assert!(app.selected_branch().is_none());
        assert!(app.selected_remote_branch().is_none());
        assert!(app.selected_commit().is_none());
        assert!(app.selected_graph_ref().is_none());
        assert!(app.selected_graph_label().is_none());
        assert!(app.sync_target_display().is_none());

        app.move_selection(1);
        assert!(app.selected_branch.is_none());
        app.set_branch_panel(BranchPanel::Remote);
        app.move_selection(-1);
        assert!(app.selected_remote_branch.is_none());
        app.move_commit_selection(1);
        app.advance_graph_scroll();
        assert_eq!(app.selected_commit, 0);
        assert_eq!(app.graph_scroll_offset, 0);

        app.open_picker();
        assert!(!app.picker_open);
        assert_eq!(app.message_kind, MessageKind::Warning);
        app.open_remote_picker();
        assert!(!app.remote_picker_open);
        app.open_commit_actions();
        assert!(!app.commit_actions_open);
        app.open_branch_creator();
        assert!(!app.branch_create_open);

        app.push_branch_search_char('x');
        app.pop_branch_search_char();
        assert!(!app.confirm_branch_search());
        app.open_branch_search();
        app.push_branch_search_char('x');
        app.push_branch_search_char('\n');
        app.pop_branch_search_char();
        assert!(!app.confirm_branch_search());
        app.close_branch_search();

        app.push_branch_create_char('x');
        app.pop_branch_create_char();
        assert!(app.branch_create_request().is_none());
        app.open_branch_creator_from_source("main".to_string(), theme::SUCCESS);
        app.push_branch_create_char(' ');
        assert!(app.branch_create_request().is_none());
        app.push_branch_create_char('x');
        assert_eq!(
            app.branch_create_request(),
            Some(("x".to_string(), "main".to_string()))
        );
        app.pop_branch_create_char();
        app.close_branch_creator();

        app.open_help();
        app.open_help();
        app.select_view(View::Branches);
        app.close_help();
        app.close_help();
        assert_eq!(app.view, View::Branches);

        app.status = Some(RepoStatus {
            branch_name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            ahead: 0,
            behind: 0,
            files: Vec::new(),
        });
        assert_eq!(app.sync_target_display().as_deref(), Some("origin/main"));
        app.start_loading("noop");
        app.advance_loading_frame();
        assert_eq!(app.loading_frame, 0);
        app.stop_loading();

        let summary = crate::domain::CommitSummary {
            hash: "abc".to_string(),
            author: "Marcos".to_string(),
            date: "2026-08-09".to_string(),
            subject: "snapshot".to_string(),
        };
        let history =
            crate::domain::BranchHistory::from_graph(vec![crate::domain::GraphLine::Commit {
                graph: "*".to_string(),
                summary: summary.clone(),
            }]);
        app.apply_snapshot(crate::domain::RepoSnapshot {
            status: RepoStatus {
                branch_name: "main".to_string(),
                upstream: None,
                ahead: 0,
                behind: 0,
                files: Vec::new(),
            },
            branches: vec![BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: None,
                commit: "abc".to_string(),
                subject: "snapshot".to_string(),
                kind: BranchKind::Local,
            }],
            history: history.clone(),
            selected_branch: Some("main".to_string()),
        });
        assert_eq!(app.selected_commit().unwrap().subject, "snapshot");

        app.graph_scroll_offset = 12;
        app.selected_commit = 8;
        app.apply_graph_history(history);
        assert_eq!(app.selected_commit, 0);
        assert_eq!(app.graph_scroll_offset, 0);
    }
}
