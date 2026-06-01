mod app;
mod controller;
mod layout;
mod operations;
mod theme;
mod widgets;

use crossterm::{
    event, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use crate::git::GitClient;

pub fn run(client: GitClient) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }
    let _cleanup = TerminalCleanup;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut controller = controller::TuiController::new(client);
    if let Err(error) = controller.refresh() {
        controller.app_mut().set_feedback(
            format!("Unable to load repository: {error}"),
            app::MessageKind::Error,
        );
    }

    let result = loop {
        controller.poll_operation()?;
        controller.tick();
        terminal.draw(|frame| controller.app().render(frame))?;

        if event::poll(Duration::from_millis(150))? {
            if controller.handle_event(event::read()?)? {
                break Ok(());
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
}
