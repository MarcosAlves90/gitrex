use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Clone, Parser)]
#[command(name = "gitrex", version, about = "Terminal-first git manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Status,
    Branch,
    Log {
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },
    Checkout {
        target: String,
    },
    Switch {
        target: String,
    },
    CreateBranch {
        name: String,
        #[arg(short, long)]
        from: Option<String>,
    },
    Clone {
        repository: String,
        directory: Option<PathBuf>,
    },
    Fetch {
        remote: Option<String>,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        remote: Option<String>,
        branch: Option<String>,
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
    },
    Tui,
}
