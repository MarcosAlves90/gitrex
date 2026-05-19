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
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        remote: Option<String>,
        branch: Option<String>,
    },
    Tui,
}
