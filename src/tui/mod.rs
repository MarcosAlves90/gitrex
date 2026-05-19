mod app;
mod layout;
mod widgets;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
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
    let mut app = app::App::new(client);
    if let Err(error) = app.refresh() {
        app.set_message(format!("Unable to load repository: {error}"));
    }

    let result = loop {
        terminal.draw(|frame| app.render(frame))?;

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) => break Ok(()),
                Event::Key(key) if matches!(key.code, KeyCode::Char('r')) => {
                    if let Err(error) = app.refresh() {
                        app.set_message(format!("Refresh failed: {error}"));
                    }
                }
                Event::Key(key) if matches!(key.code, KeyCode::Char('1')) => {
                    app.select_view(app::View::Status);
                }
                Event::Key(key) if matches!(key.code, KeyCode::Char('2')) => {
                    app.select_view(app::View::Branches);
                }
                Event::Key(key) if matches!(key.code, KeyCode::Char('3')) => {
                    app.select_view(app::View::Log);
                }
                _ => {}
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
