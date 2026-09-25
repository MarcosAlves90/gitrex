mod args;
pub mod output;
pub(crate) mod protocol;

use crate::app::operations as mutation_ops;
use crate::app::repository_context::{
    self as context_ops, ContextLimits, DiffOptions, DiffSelection,
};
use crate::domain::{MutationRequest, OperationPreconditions};
use crate::git::GitClient;

pub use args::{Cli, Commands, MutationOutputArgs, OutputFormat, PlannedMutationOutputArgs};

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
        Some(Commands::Checkout { target, options }) => run_mutation(
            &client,
            MutationRequest::Checkout {
                target: target.clone(),
            },
            &options,
            false,
            &format!("checked out {target}"),
        ),
        Some(Commands::Switch { target, options }) => run_mutation(
            &client,
            MutationRequest::Switch {
                target: target.clone(),
            },
            &options.mutation,
            options.dry_run,
            &format!("switched to {target}"),
        ),
        Some(Commands::CreateBranch {
            name,
            from,
            options,
        }) => {
            let message = match from.as_deref() {
                Some(source) => format!("created {name} from {source}"),
                None => format!("created {name}"),
            };
            run_mutation(
                &client,
                MutationRequest::CreateBranch { name, from },
                &options.mutation,
                options.dry_run,
                &message,
            )
        }
        Some(Commands::Clone {
            repository,
            directory,
        }) => {
            client.clone_repository(&repository, directory.as_deref())?;
            output::print_message("clone complete");
            Ok(())
        }
        Some(Commands::Fetch { remote, options }) => run_mutation(
            &client,
            MutationRequest::Fetch { remote },
            &options.mutation,
            options.dry_run,
            "fetch complete",
        ),
        Some(Commands::Pull {
            remote,
            branch,
            options,
        }) => run_mutation(
            &client,
            MutationRequest::Pull { remote, branch },
            &options.mutation,
            options.dry_run,
            "pull complete",
        ),
        Some(Commands::Push {
            remote,
            branch,
            options,
        }) => run_mutation(
            &client,
            MutationRequest::Push { remote, branch },
            &options.mutation,
            options.dry_run,
            "push complete",
        ),
        Some(Commands::Cleanup {
            base,
            exclusions,
            remotes,
            yes,
            options,
        }) => execute_cleanup(&client, base, exclusions, remotes, yes, options),
        Some(Commands::Tui) | None => {
            output::print_help_hint();
            Ok(())
        }
    }
}

fn run_mutation(
    client: &GitClient,
    request: MutationRequest,
    options: &MutationOutputArgs,
    dry_run: bool,
    success_message: &str,
) -> anyhow::Result<()> {
    let operation = request.name();
    let preconditions = OperationPreconditions {
        expect_head: options.expect_head.clone(),
        expect_branch: options.expect_branch.clone(),
        expect_upstream: options.expect_upstream.clone(),
    };
    if dry_run {
        return match mutation_ops::plan_mutation(client, &request, &preconditions) {
            Ok(plan) => match options.format {
                OutputFormat::Json => protocol::print_operation_plan(operation, plan),
                OutputFormat::Text => {
                    output::print_operation_plan(&plan);
                    Ok(())
                }
            },
            Err(error) => {
                if options.format == OutputFormat::Json {
                    protocol::print_failure(operation, &error)?;
                }
                Err(anyhow::Error::new(error))
            }
        };
    }

    let execution = match mutation_ops::execute_mutation(client, &request, &preconditions) {
        Ok(execution) => execution,
        Err(error) => {
            if options.format == OutputFormat::Json {
                protocol::print_failure(operation, &error)?;
            }
            return Err(anyhow::Error::new(error));
        }
    };
    let failure_message = execution
        .failure
        .as_ref()
        .map(|failure| failure.error.to_string());
    if options.format == OutputFormat::Json {
        protocol::print_operation_execution(operation, execution)?;
    } else if let Some(message) = &failure_message {
        output::print_message(message);
    } else {
        output::print_message(success_message);
    }
    if let Some(message) = failure_message {
        anyhow::bail!(message);
    }
    Ok(())
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
    options: PlannedMutationOutputArgs,
) -> anyhow::Result<()> {
    let base = base.unwrap_or_else(|| String::from("HEAD"));
    let request = MutationRequest::Cleanup {
        base: base.clone(),
        branches: None,
        exclusions: exclusions.clone(),
        remotes: remotes.clone(),
    };
    let preconditions = OperationPreconditions {
        expect_head: options.mutation.expect_head.clone(),
        expect_branch: options.mutation.expect_branch.clone(),
        expect_upstream: options.mutation.expect_upstream.clone(),
    };
    if options.dry_run || !yes {
        let plan = match mutation_ops::plan_mutation(client, &request, &preconditions) {
            Ok(plan) => plan,
            Err(error) => {
                if options.mutation.format == OutputFormat::Json {
                    protocol::print_failure("cleanup", &error)?;
                }
                return Err(anyhow::Error::new(error));
            }
        };
        if options.mutation.format == OutputFormat::Json {
            return protocol::print_operation_plan("cleanup", plan);
        }
        output::print_message(&format!("Merged local branches reachable from {base}:"));
        if plan.expected_local_effects.is_empty() {
            output::print_message("No merged local branches found.");
        } else {
            for effect in &plan.expected_local_effects {
                if let Some(branch) = effect.target.strip_prefix("refs/heads/") {
                    output::print_message(&format!("  {branch}"));
                }
            }
        }
        if options.dry_run {
            output::print_operation_plan(&plan);
        } else {
            output::print_message("Preview only; pass --yes to delete these branches.");
        }
        return Ok(());
    }

    let plan = mutation_ops::plan_mutation(client, &request, &preconditions);
    let planned_branches = match plan {
        Ok(plan) => plan
            .expected_local_effects
            .iter()
            .filter_map(|effect| effect.target.strip_prefix("refs/heads/").map(str::to_owned))
            .collect::<Vec<_>>(),
        Err(error) => {
            if options.mutation.format == OutputFormat::Json {
                protocol::print_failure("cleanup", &error)?;
            }
            return Err(anyhow::Error::new(error));
        }
    };
    let execution = match mutation_ops::execute_mutation(client, &request, &preconditions) {
        Ok(execution) => execution,
        Err(error) => {
            if options.mutation.format == OutputFormat::Json {
                protocol::print_failure("cleanup", &error)?;
            }
            return Err(anyhow::Error::new(error));
        }
    };
    let failure_message = execution
        .failure
        .as_ref()
        .map(|failure| failure.error.to_string());
    if options.mutation.format == OutputFormat::Json {
        protocol::print_operation_execution("cleanup", execution)?;
    } else {
        let deleted = execution.receipt.confirmed_local_effects.len();
        let skipped = planned_branches.len().saturating_sub(deleted);
        for branch in &planned_branches {
            let label = if execution
                .receipt
                .confirmed_local_effects
                .iter()
                .any(|effect| effect.target == format!("refs/heads/{branch}"))
            {
                "deleted"
            } else {
                "skipped"
            };
            output::print_message(&format!("{label}: {branch}"));
        }
        output::print_message(&format!(
            "Cleanup complete: {deleted} deleted, {skipped} skipped, {} failed.",
            usize::from(failure_message.is_some())
        ));
    }
    if let Some(message) = failure_message {
        anyhow::bail!(message);
    }
    Ok(())
}
