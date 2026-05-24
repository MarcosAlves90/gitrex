use super::{CommitSummary, GraphLine};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchHistory {
    pub commits: Vec<CommitSummary>,
    pub graph: Vec<GraphLine>,
}

impl BranchHistory {
    pub fn from_graph(graph: Vec<GraphLine>) -> Self {
        let commits = graph
            .iter()
            .filter_map(|line| match line {
                GraphLine::Commit { summary, .. } => Some(summary.clone()),
                GraphLine::Connector { .. } => None,
            })
            .collect();

        Self { commits, graph }
    }
}
