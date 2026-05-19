use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::{
    cli::output,
    domain::{BranchInfo, CommitSummary, RepoStatus},
    git::GitClient,
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
pub enum Intent {
    None,
    Quit,
    Refresh,
    SelectView(View),
    MoveSelection(isize),
    CheckoutSelected,
    SwitchSelected,
    PullCurrent,
    PushCurrent,
}

pub struct App {
    client: GitClient,
    view: View,
    status: Option<RepoStatus>,
    branches: Vec<BranchInfo>,
    log: Vec<CommitSummary>,
    selected_branch: usize,
    message: String,
    message_kind: MessageKind,
}

impl App {
    pub fn new(client: GitClient) -> Self {
        Self {
            client,
            view: View::Branches,
            status: None,
            branches: Vec::new(),
            log: Vec::new(),
            selected_branch: 0,
            message: String::from("Press q to quit, r to refresh."),
            message_kind: MessageKind::Info,
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.status = Some(self.client.status()?);
        self.branches = self.client.branches()?;
        self.log = self.client.log(12)?;
        self.selected_branch = self
            .branches
            .iter()
            .position(|branch| branch.current && matches!(branch.kind, crate::domain::BranchKind::Local))
            .unwrap_or(0);
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

    fn intent_for_key(&self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('1') => Intent::SelectView(View::Status),
            KeyCode::Char('2') => Intent::SelectView(View::Branches),
            KeyCode::Char('3') => Intent::SelectView(View::Log),
            KeyCode::Char('j') | KeyCode::Down => Intent::MoveSelection(1),
            KeyCode::Char('k') | KeyCode::Up => Intent::MoveSelection(-1),
            KeyCode::Enter | KeyCode::Char('c') => Intent::CheckoutSelected,
            KeyCode::Char('s') => Intent::SwitchSelected,
            KeyCode::Char('p') => Intent::PullCurrent,
            KeyCode::Char('P') => Intent::PushCurrent,
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
            Intent::CheckoutSelected => {
                self.checkout_selected()?;
                Ok(false)
            }
            Intent::SwitchSelected => {
                self.switch_selected()?;
                Ok(false)
            }
            Intent::PullCurrent => {
                self.pull_current()?;
                Ok(false)
            }
            Intent::PushCurrent => {
                self.push_current()?;
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

    fn checkout_selected(&mut self) -> anyhow::Result<()> {
        let branch = match self.selected_branch() {
            Some(branch) => branch.name.clone(),
            None => {
                self.set_feedback("No branch selected.", MessageKind::Warning);
                return Ok(());
            }
        };

        self.client.checkout(&branch)?;
        self.refresh()?;
        self.set_feedback(format!("Checked out {branch}"), MessageKind::Success);
        Ok(())
    }

    fn switch_selected(&mut self) -> anyhow::Result<()> {
        let branch = match self.selected_branch() {
            Some(branch) => branch.name.clone(),
            None => {
                self.set_feedback("No branch selected.", MessageKind::Warning);
                return Ok(());
            }
        };

        self.client.switch(&branch)?;
        self.refresh()?;
        self.set_feedback(format!("Switched to {branch}"), MessageKind::Success);
        Ok(())
    }

    fn pull_current(&mut self) -> anyhow::Result<()> {
        let branch = self.status.as_ref().map(|status| status.branch_name.clone());
        self.client.pull(None, branch.as_deref())?;
        self.refresh()?;
        self.set_feedback("Pull complete.", MessageKind::Success);
        Ok(())
    }

    fn push_current(&mut self) -> anyhow::Result<()> {
        let branch = self.status.as_ref().map(|status| status.branch_name.clone());
        self.client.push(None, branch.as_deref())?;
        self.refresh()?;
        self.set_feedback("Push complete.", MessageKind::Success);
        Ok(())
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
                    .title("Branches")
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
        Paragraph::new(self.message.clone())
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

    fn branch_state(&self) -> ListState {
        let mut state = ListState::default();
        if !self.local_branches().is_empty() {
            state.select(Some(self.selected_branch.min(self.local_branches().len().saturating_sub(1))));
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Intent, MessageKind, View};
    use crate::{
        domain::{BranchInfo, BranchKind},
        git::GitClient,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn key_map_goes_to_branch_actions() {
        let app = App::new(GitClient::new());
        assert_eq!(
            app.intent_for_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Intent::MoveSelection(1)
        );
        assert_eq!(
            app.intent_for_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            Intent::CheckoutSelected
        );
        assert_eq!(
            app.intent_for_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)),
            Intent::PushCurrent
        );
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
}
