pub mod branch;
pub mod error;
pub mod history;
pub mod log;
pub mod status;

pub use branch::{
    build_branch_catalog, BranchCatalog, BranchInfo, BranchKind, LocalBranchEntry,
    RemoteBranchGroup,
};
pub use error::{GitError, Result};
pub use history::BranchHistory;
pub use log::{CommitSummary, GraphLine};
pub use status::{RepoStatus, StatusEntry};
