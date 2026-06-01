use super::{BranchHistory, BranchInfo, RepoStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    pub status: RepoStatus,
    pub branches: Vec<BranchInfo>,
    pub history: BranchHistory,
    pub selected_branch: Option<String>,
}
