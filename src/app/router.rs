use std::io::IsTerminal;

use crate::cli::{Cli, Commands};
use crate::git::GitClient;
use crate::tui;

use super::startup::{runtime_mode, RuntimeMode};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mode = runtime_mode(interactive);
    let client = GitClient::new();

    match (mode, cli.command) {
        (_, Some(Commands::Tui)) => tui::run(client),
        (RuntimeMode::Tui, None) => tui::run(client),
        (_, command) => crate::cli::execute(command, client),
    }
}
