#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectScope {
    Minimal,
    Change,
    Branches,
    Full,
}

impl InspectScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Change => "change",
            Self::Branches => "branches",
            Self::Full => "full",
        }
    }

    pub fn includes_paths(self) -> bool {
        matches!(self, Self::Change | Self::Full)
    }

    pub fn includes_branches(self) -> bool {
        matches!(self, Self::Branches | Self::Full)
    }

    pub fn includes_history(self) -> bool {
        matches!(self, Self::Full)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub root: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadContext {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamContext {
    pub reference: String,
    pub commit: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPath {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathGroup {
    pub count: usize,
    pub paths: Option<Vec<ContextPath>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeContext {
    pub clean: bool,
    pub staged: PathGroup,
    pub unstaged: PathGroup,
    pub untracked: PathGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictContext {
    pub has_conflicts: bool,
    pub count: usize,
    pub paths: Option<Vec<ContextPath>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchContext {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub commit: String,
    pub subject: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchList {
    pub count: usize,
    pub branches: Vec<BranchContext>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitList {
    pub count: usize,
    pub commits: Vec<ContextCommit>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryContext {
    pub scope: String,
    pub repository: RepositoryIdentity,
    pub head: HeadContext,
    pub upstream: Option<UpstreamContext>,
    pub working_tree: WorkingTreeContext,
    pub conflicts: ConflictContext,
    pub branches: Option<BranchList>,
    pub history: Option<CommitList>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub text: String,
    pub returned_bytes: usize,
    pub total_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub mode: String,
    pub left_commit: Option<String>,
    pub right_commit: Option<String>,
    pub changed_files: Vec<ChangedFile>,
    pub changed_file_count: usize,
    pub changed_files_truncated: bool,
    pub additions: u64,
    pub deletions: u64,
    pub patch: Option<Patch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub timestamp: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowReport {
    pub commit: CommitDetails,
    pub diff: DiffReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareReport {
    pub left_reference: String,
    pub left_commit: String,
    pub right_reference: String,
    pub right_commit: String,
    pub merge_bases: Vec<String>,
    pub ahead: u64,
    pub behind: u64,
    pub left_only_commits: Vec<ContextCommit>,
    pub left_only_commit_count: u64,
    pub left_only_commits_truncated: bool,
    pub right_only_commits: Vec<ContextCommit>,
    pub right_only_commit_count: u64,
    pub right_only_commits_truncated: bool,
    pub diff: DiffReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitReference {
    pub reference: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeContextReport {
    pub base: CommitReference,
    pub head: CommitReference,
    pub merge_bases: Vec<String>,
    pub commits: CommitList,
    pub diff: DiffReport,
    pub working_tree: WorkingTreeContext,
    pub conflicts: ConflictContext,
}
