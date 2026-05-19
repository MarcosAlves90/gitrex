mod args;
pub mod output;

use crate::git::GitClient;

pub use args::{Cli, Commands};

pub fn execute(command: Option<Commands>, client: GitClient) -> anyhow::Result<()> {
    match command {
        Some(Commands::Status) => output::print_status(&client.status()?),
        Some(Commands::Branch) => output::print_branches(&client.branches()?),
        Some(Commands::Log { limit }) => output::print_log(&client.log(limit)?),
        Some(Commands::Checkout { target }) => {
            client.checkout(&target)?;
            output::print_message(&format!("checked out {target}"));
        }
        Some(Commands::Switch { target }) => {
            client.switch(&target)?;
            output::print_message(&format!("switched to {target}"));
        }
        Some(Commands::Clone {
            repository,
            directory,
        }) => {
            client.clone(&repository, directory.as_deref())?;
            output::print_message("clone complete");
        }
        Some(Commands::Pull { remote, branch }) => {
            client.pull(remote.as_deref(), branch.as_deref())?;
            output::print_message("pull complete");
        }
        Some(Commands::Push { remote, branch }) => {
            client.push(remote.as_deref(), branch.as_deref())?;
            output::print_message("push complete");
        }
        Some(Commands::Tui) | None => {
            output::print_help_hint();
        }
    }

    Ok(())
}
