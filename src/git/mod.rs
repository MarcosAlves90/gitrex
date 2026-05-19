mod branch;
mod command;
mod log;
mod status;

pub use branch::{list_branches, parse_branch_lines};
pub use command::GitClient;
pub use log::{parse_log_lines, read_log};
pub use status::{parse_status_output, read_status};
