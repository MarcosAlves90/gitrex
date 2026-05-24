#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphLine {
    Commit {
        graph: String,
        summary: CommitSummary,
    },
    Connector {
        graph: String,
    },
}
