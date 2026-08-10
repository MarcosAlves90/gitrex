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

#[cfg(test)]
mod tests {
    use super::BranchHistory;
    use crate::domain::{CommitSummary, GraphLine};

    #[test]
    fn from_graph_extracts_only_commit_rows_and_preserves_graph() {
        let first = CommitSummary {
            hash: "11111111".to_string(),
            author: "A".to_string(),
            date: "2026-08-09".to_string(),
            subject: "first".to_string(),
        };
        let second = CommitSummary {
            hash: "22222222".to_string(),
            author: "B".to_string(),
            date: "2026-08-09".to_string(),
            subject: "second".to_string(),
        };
        let graph = vec![
            GraphLine::Commit {
                graph: "*".to_string(),
                summary: first.clone(),
            },
            GraphLine::Connector {
                graph: "|".to_string(),
            },
            GraphLine::Commit {
                graph: "*".to_string(),
                summary: second.clone(),
            },
        ];

        let history = BranchHistory::from_graph(graph.clone());

        assert_eq!(history.commits, vec![first, second]);
        assert_eq!(history.graph, graph);
    }
}
