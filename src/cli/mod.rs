mod args;
pub mod output;
pub(crate) mod protocol;

use crate::app::repository_context::{
    self as context_ops, ContextLimits, DiffOptions, DiffSelection,
};
use crate::domain::branch::{BranchCleanupOutcome, BranchCleanupState};
use crate::git::GitClient;

pub use args::{Cli, Commands, OutputFormat};

pub fn execute(command: Option<Commands>, client: GitClient) -> anyhow::Result<()> {
    match command {
        Some(Commands::Status { format }) => execute_read_only(
            client.status(),
            format,
            "status",
            protocol::StatusData::from,
            output::print_status,
        ),
        Some(Commands::Branch { format }) => execute_read_only(
            client.branches(),
            format,
            "branch",
            protocol::BranchData::from,
            |branches| output::print_branches(branches),
        ),
        Some(Commands::Log { limit, format }) => execute_read_only(
            client.log(limit),
            format,
            "log",
            protocol::LogData::from,
            |entries| output::print_log(entries),
        ),
        Some(Commands::Inspect {
            scope,
            max_paths,
            history_limit,
            max_branches,
            format,
        }) => {
            let limits = ContextLimits {
                max_paths,
                history_limit,
                max_branches,
                ..ContextLimits::default()
            };
            execute_read_only(
                context_ops::inspect(&client, scope.into(), limits),
                format,
                "inspect",
                protocol::InspectData::from,
                output::print_inspect,
            )
        }
        Some(Commands::Diff {
            staged,
            base,
            from,
            to,
            max_paths,
            max_patch_bytes,
            include_patch,
            format,
        }) => {
            let selection = match (staged, base, from, to) {
                (true, _, _, _) => DiffSelection::Staged,
                (false, Some(base), _, _) => DiffSelection::Base(base),
                (false, None, Some(from), Some(to)) => DiffSelection::Refs { from, to },
                _ => DiffSelection::Worktree,
            };
            execute_read_only(
                context_ops::diff(
                    &client,
                    DiffOptions {
                        selection,
                        limits: ContextLimits {
                            max_paths,
                            max_patch_bytes,
                            ..ContextLimits::default()
                        },
                        include_patch: include_patch || format == OutputFormat::Text,
                    },
                ),
                format,
                "diff",
                protocol::DiffData::from,
                output::print_diff,
            )
        }
        Some(Commands::Show {
            commit,
            max_paths,
            max_patch_bytes,
            include_patch,
            format,
        }) => execute_read_only(
            context_ops::show(
                &client,
                &commit,
                ContextLimits {
                    max_paths,
                    max_patch_bytes,
                    ..ContextLimits::default()
                },
                include_patch || format == OutputFormat::Text,
            ),
            format,
            "show",
            protocol::ShowData::from,
            output::print_show,
        ),
        Some(Commands::Compare {
            left,
            right,
            max_paths,
            max_commits,
            format,
        }) => execute_read_only(
            context_ops::compare(
                &client,
                &left,
                &right,
                ContextLimits {
                    max_paths,
                    max_commits,
                    ..ContextLimits::default()
                },
            ),
            format,
            "compare",
            protocol::CompareData::from,
            output::print_compare,
        ),
        Some(Commands::ChangeContext {
            base,
            max_paths,
            max_commits,
            format,
        }) => execute_read_only(
            context_ops::change_context(
                &client,
                &base,
                ContextLimits {
                    max_paths,
                    max_commits,
                    ..ContextLimits::default()
                },
            ),
            format,
            "change-context",
            protocol::ChangeContextData::from,
            output::print_change_context,
        ),
        Some(Commands::Capabilities { format }) => {
            let capabilities = protocol::capabilities();
            match format {
                OutputFormat::Text => protocol::print_capabilities_text(&capabilities),
                OutputFormat::Json => protocol::print_success("capabilities", capabilities)?,
            }
            Ok(())
        }
        Some(Commands::Checkout { target }) => {
            client.checkout(&target)?;
            output::print_message(&format!("checked out {target}"));
            Ok(())
        }
        Some(Commands::Switch { target }) => {
            client.switch(&target)?;
            output::print_message(&format!("switched to {target}"));
            Ok(())
        }
        Some(Commands::CreateBranch { name, from }) => {
            client.create_branch(&name, from.as_deref())?;
            match from {
                Some(source) => output::print_message(&format!("created {name} from {source}")),
                None => output::print_message(&format!("created {name}")),
            }
            Ok(())
        }
        Some(Commands::Clone {
            repository,
            directory,
        }) => {
            client.clone_repository(&repository, directory.as_deref())?;
            output::print_message("clone complete");
            Ok(())
        }
        Some(Commands::Fetch { remote }) => {
            client.fetch(remote.as_deref())?;
            output::print_message("fetch complete");
            Ok(())
        }
        Some(Commands::Pull { remote, branch }) => {
            client.pull(remote.as_deref(), branch.as_deref())?;
            output::print_message("pull complete");
            Ok(())
        }
        Some(Commands::Push { remote, branch }) => {
            client.push(remote.as_deref(), branch.as_deref())?;
            output::print_message("push complete");
            Ok(())
        }
        Some(Commands::Cleanup {
            base,
            exclusions,
            remotes,
            yes,
        }) => {
            execute_cleanup(&client, base, exclusions, remotes, yes)?;
            Ok(())
        }
        Some(Commands::Tui) | None => {
            output::print_help_hint();
            Ok(())
        }
    }
}

fn execute_read_only<T, U>(
    result: crate::domain::Result<T>,
    format: OutputFormat,
    operation: &'static str,
    to_protocol: impl FnOnce(T) -> U,
    print_text: impl FnOnce(&T),
) -> anyhow::Result<()>
where
    U: serde::Serialize,
{
    match result {
        Ok(value) => match format {
            OutputFormat::Text => {
                print_text(&value);
                Ok(())
            }
            OutputFormat::Json => protocol::print_success(operation, to_protocol(value)),
        },
        Err(error) => {
            if format == OutputFormat::Json {
                protocol::print_failure(operation, &error)?;
            }
            Err(anyhow::Error::new(error))
        }
    }
}

fn execute_cleanup(
    client: &GitClient,
    base: Option<String>,
    exclusions: Vec<String>,
    remotes: Vec<String>,
    yes: bool,
) -> anyhow::Result<()> {
    let base = base.unwrap_or_else(|| String::from("HEAD"));
    let candidates = client.merged_local_branches(&base, &exclusions, &remotes)?;

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
                .cleanup_local_branch(&candidate.name, &base, &exclusions, &remotes)
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
