#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub commit: String,
    pub subject: String,
    pub kind: BranchKind,
}
