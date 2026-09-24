mod args;
pub mod output;

use crate::domain::branch::{BranchCleanupOutcome, BranchCleanupState};
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
        Some(Commands::CreateBranch { name, from }) => {
            client.create_branch(&name, from.as_deref())?;
            match from {
                Some(source) => output::print_message(&format!("created {name} from {source}")),
                None => output::print_message(&format!("created {name}")),
            }
        }
        Some(Commands::Clone {
            repository,
            directory,
        }) => {
            client.clone_repository(&repository, directory.as_deref())?;
            output::print_message("clone complete");
        }
        Some(Commands::Fetch { remote }) => {
            client.fetch(remote.as_deref())?;
            output::print_message("fetch complete");
        }
        Some(Commands::Pull { remote, branch }) => {
            client.pull(remote.as_deref(), branch.as_deref())?;
            output::print_message("pull complete");
        }
        Some(Commands::Push { remote, branch }) => {
            client.push(remote.as_deref(), branch.as_deref())?;
            output::print_message("push complete");
        }
        Some(Commands::Cleanup {
            base,
            exclusions,
            remote,
            yes,
        }) => execute_cleanup(&client, base, exclusions, remote, yes)?,
        Some(Commands::Tui) | None => {
            output::print_help_hint();
        }
    }

    Ok(())
}

fn execute_cleanup(
    client: &GitClient,
    base: Option<String>,
    exclusions: Vec<String>,
    remote: Option<String>,
    yes: bool,
) -> anyhow::Result<()> {
    let base = base.unwrap_or_else(|| String::from("HEAD"));
    let candidates = client.merged_local_branches(&base, &exclusions, remote.as_deref())?;

    output::print_message(&format!("Merged local branches reachable from {base}:"));
    if candidates.is_empty() {
        output::print_message("No merged local branches found.");
    } else {
        for candidate in &candidates {
            output::print_message(&format!("  {}", candidate.name));
        }
    }

    if !yes {
        output::print_message("Preview only; pass --yes to delete these branches.");
        return Ok(());
    }

    let outcomes = candidates
        .iter()
        .map(|candidate| {
            client
                .cleanup_local_branch(&candidate.name, &base, &exclusions, remote.as_deref())
                .unwrap_or_else(|error| BranchCleanupOutcome {
                    branch: candidate.name.clone(),
                    state: BranchCleanupState::Failed,
                    detail: Some(error.to_string()),
                })
        })
        .collect::<Vec<_>>();

    let mut deleted = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for outcome in &outcomes {
        let (label, detail) = match outcome.state {
            BranchCleanupState::Deleted => {
                deleted += 1;
                ("deleted", None)
            }
            BranchCleanupState::Skipped => {
                skipped += 1;
                ("skipped", outcome.detail.as_deref())
            }
            BranchCleanupState::Failed => {
                failed += 1;
                ("failed", outcome.detail.as_deref())
            }
        };
        match detail {
            Some(detail) => {
                output::print_message(&format!("{label}: {} ({detail})", outcome.branch))
            }
            None => output::print_message(&format!("{label}: {}", outcome.branch)),
        }
    }
    output::print_message(&format!(
        "Cleanup complete: {deleted} deleted, {skipped} skipped, {failed} failed."
    ));

    if failed > 0 {
        anyhow::bail!("{failed} local branch cleanup operation(s) failed");
    }
    Ok(())
}
