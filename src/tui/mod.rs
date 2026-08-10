mod app;
mod branching;
mod controller;
mod layout;
mod operation_flow;
mod operations;
mod theme;
mod widgets;

use crossterm::{
    event, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::mpsc;
use std::thread;
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
    let refresh_client = <GitClient as Clone>::clone(&client);
    let mut controller = controller::TuiController::new(client);

    controller.app_mut().start_loading("Loading repository");

    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = snapshot_tx.send(refresh_client.snapshot());
    });

    loop {
        controller.tick();
        terminal.draw(|frame| controller.app_mut().render(frame))?;

        match snapshot_rx.try_recv() {
            Ok(Ok(snapshot)) => {
                controller.app_mut().apply_snapshot(snapshot);
                controller
                    .app_mut()
                    .set_feedback("Repository loaded.", app::MessageKind::Success);
                controller.app_mut().stop_loading();
                break;
            }
            Ok(Err(error)) => {
                controller.app_mut().set_feedback(
                    format!("Unable to load repository: {error}"),
                    app::MessageKind::Error,
                );
                controller.app_mut().stop_loading();
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                controller.app_mut().set_feedback(
                    "Repository loading aborted unexpectedly.",
                    app::MessageKind::Error,
                );
                controller.app_mut().stop_loading();
                break;
            }
        }

        if event::poll(Duration::from_millis(80))? && controller.handle_event(event::read()?)? {
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            terminal.show_cursor()?;
            return Ok(());
        }
    }

    let result = loop {
        controller.poll_operation()?;
        controller.tick();
        terminal.draw(|frame| controller.app_mut().render(frame))?;

        if event::poll(Duration::from_millis(150))? && controller.handle_event(event::read()?)? {
            break Ok(());
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
