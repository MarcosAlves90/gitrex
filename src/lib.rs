#[cfg(test)]
pub mod test_support;

pub mod app;
pub mod cli;
pub mod domain;
pub mod git;
pub mod tui;

pub use app::run;
