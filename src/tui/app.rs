use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::{
    cli::output,
    domain::{BranchInfo, CommitSummary, RepoStatus},
    git::GitClient,
};

use super::{layout, widgets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Status,
    Branches,
    Log,
}

pub struct App {
    client: GitClient,
    view: View,
    status: Option<RepoStatus>,
    branches: Vec<BranchInfo>,
    log: Vec<CommitSummary>,
    message: String,
}

impl App {
    pub fn new(client: GitClient) -> Self {
        Self {
            client,
            view: View::Status,
            status: None,
            branches: Vec::new(),
            log: Vec::new(),
            message: String::from("Press q to quit, r to refresh."),
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.status = Some(self.client.status()?);
        self.branches = self.client.branches()?;
        self.log = self.client.log(12)?;
        self.message = String::from("Repository refreshed.");
        Ok(())
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn select_view(&mut self, view: View) {
        self.view = view;
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let chunks = layout::dashboard(frame.area());
        let status_block = self.render_status();
        let list_block = self.render_secondary();
        let shortcuts_block = self.render_shortcuts();
        let footer = Paragraph::new(self.message.clone())
            .block(Block::default().title("Message").borders(Borders::ALL));

        frame.render_widget(status_block, chunks[0]);
        frame.render_widget(list_block, chunks[1]);
        frame.render_widget(shortcuts_block, chunks[2]);
        frame.render_widget(footer, chunks[3]);
    }

    fn render_status(&self) -> Paragraph<'_> {
        let text = self
            .status
            .as_ref()
            .map(output::render_status_summary)
            .unwrap_or_else(|| "no repository loaded".to_string());
        Paragraph::new(text).block(Block::default().title("Status").borders(Borders::ALL))
    }

    fn render_secondary(&self) -> List<'_> {
        let items = match self.view {
            View::Status => output::render_status_entries(
                self.status
                    .as_ref()
                    .map(|status| status.files.as_slice())
                    .unwrap_or(&[] as &[crate::domain::StatusEntry]),
            ),
            View::Branches => output::render_branch_preview(&self.branches),
            View::Log => output::render_log_preview(&self.log),
        };

        let title = match self.view {
            View::Status => "Changes",
            View::Branches => "Branches",
            View::Log => "Log",
        };

        List::new(items.into_iter().map(ListItem::new))
            .block(Block::default().title(title).borders(Borders::ALL))
    }

    fn render_shortcuts(&self) -> List<'_> {
        let shortcuts = widgets::shortcuts_line();
        List::new([shortcuts].into_iter().map(ListItem::new))
            .block(Block::default().title("Shortcuts").borders(Borders::ALL))
    }
}
