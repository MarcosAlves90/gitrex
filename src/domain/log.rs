#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}
