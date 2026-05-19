#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoStatus {
    pub branch_name: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub code: String,
    pub path: String,
}
