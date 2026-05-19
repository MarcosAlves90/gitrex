pub mod router;
pub mod startup;

use clap::Parser;

use crate::cli::Cli;

pub fn run() -> anyhow::Result<()> {
    router::run(Cli::parse())
}
