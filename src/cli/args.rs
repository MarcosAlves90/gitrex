use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(name = "gitrex", version, about = "Terminal-first git manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InspectScopeArg {
    Minimal,
    Change,
    Branches,
    Full,
}

impl From<InspectScopeArg> for crate::domain::repository_context::InspectScope {
    fn from(scope: InspectScopeArg) -> Self {
        match scope {
            InspectScopeArg::Minimal => Self::Minimal,
            InspectScopeArg::Change => Self::Change,
            InspectScopeArg::Branches => Self::Branches,
            InspectScopeArg::Full => Self::Full,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct MutationOutputArgs {
    #[arg(long)]
    pub expect_head: Option<String>,
    #[arg(long)]
    pub expect_branch: Option<String>,
    #[arg(long)]
    pub expect_upstream: Option<String>,
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Debug, Clone, Args)]
pub struct PlannedMutationOutputArgs {
    #[command(flatten)]
    pub mutation: MutationOutputArgs,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Status {
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    Branch {
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    Log {
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    #[command(about = "Inspect local repository state and bounded context")]
    Inspect {
        #[arg(long, value_enum, default_value = "full")]
        scope: InspectScopeArg,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATHS)]
        max_paths: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_HISTORY_LIMIT)]
        history_limit: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_BRANCHES)]
        max_branches: usize,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    #[command(about = "Show changed paths and an optional bounded patch")]
    Diff {
        #[arg(long, conflicts_with_all = ["base", "from", "to"])]
        staged: bool,
        #[arg(long, conflicts_with_all = ["staged", "from", "to"])]
        base: Option<String>,
        #[arg(long, requires = "to", conflicts_with_all = ["staged", "base"])]
        from: Option<String>,
        #[arg(long, requires = "from", conflicts_with_all = ["staged", "base"])]
        to: Option<String>,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATHS)]
        max_paths: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATCH_BYTES)]
        max_patch_bytes: usize,
        #[arg(long, help = "Include patch text in JSON output")]
        include_patch: bool,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    #[command(about = "Show commit metadata and its first-parent diff")]
    Show {
        commit: String,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATHS)]
        max_paths: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATCH_BYTES)]
        max_patch_bytes: usize,
        #[arg(long, help = "Include patch text in JSON output")]
        include_patch: bool,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    #[command(about = "Compare two local commits or refs")]
    Compare {
        left: String,
        right: String,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATHS)]
        max_paths: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_COMMITS)]
        max_commits: usize,
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    #[command(about = "Build a bounded view of changes since a base ref")]
    ChangeContext {
        #[arg(long, required = true)]
        base: String,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_PATHS)]
        max_paths: usize,
        #[arg(long, default_value_t = crate::app::repository_context::DEFAULT_MAX_COMMITS)]
        max_commits: usize,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Capabilities {
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },
    Checkout {
        target: String,
        #[command(flatten)]
        options: MutationOutputArgs,
    },
    Switch {
        target: String,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    CreateBranch {
        name: String,
        #[arg(short, long)]
        from: Option<String>,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    Clone {
        repository: String,
        directory: Option<PathBuf>,
    },
    Fetch {
        remote: Option<String>,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    Push {
        remote: Option<String>,
        branch: Option<String>,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    #[command(about = "Preview and safely clean up merged local branches")]
    Cleanup {
        #[arg(
            long,
            help = "Base commit or ref to compare against (defaults to HEAD)"
        )]
        base: Option<String>,
        #[arg(
            long = "exclude",
            help = "Exact local branch name to keep (may be repeated)"
        )]
        exclusions: Vec<String>,
        #[arg(
            long = "remote",
            help = "Only include branches whose configured upstream uses one of these remotes (may be repeated)"
        )]
        remotes: Vec<String>,
        #[arg(long, help = "Delete eligible branches after previewing the list")]
        yes: bool,
        #[command(flatten)]
        options: PlannedMutationOutputArgs,
    },
    Tui,
}
