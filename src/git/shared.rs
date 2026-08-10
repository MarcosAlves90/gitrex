use crate::domain::{CommitSummary, GitError, GraphLine, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCommit {
    pub id: String,
    pub summary: CommitSummary,
    pub parents: Vec<String>,
}

pub fn parse_history_records(output: &[u8]) -> Result<Vec<HistoryCommit>> {
    let fields = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    let fields = if fields.last().is_some_and(|field| field.is_empty()) {
        &fields[..fields.len().saturating_sub(1)]
    } else {
        fields.as_slice()
    };

    if fields.len() % 5 != 0 {
        return Err(GitError::Parse(String::from(
            "unexpected git log record shape",
        )));
    }

    fields
        .chunks_exact(5)
        .map(|record| {
            let id = parse_utf8(record[0])?;
            let parents = parse_utf8(record[1])?
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            let author = parse_utf8(record[2])?;
            let date = parse_utf8(record[3])?;
            let subject = parse_utf8(record[4])?;

            Ok(HistoryCommit {
                id: id.clone(),
                summary: CommitSummary {
                    hash: id,
                    author,
                    date,
                    subject,
                },
                parents,
            })
        })
        .collect()
}

fn parse_utf8(value: &[u8]) -> Result<String> {
    String::from_utf8(value.to_vec()).map_err(|_| GitError::Utf8)
}

pub fn render_graph(commits: &[HistoryCommit]) -> Vec<GraphLine> {
    let mut lanes: Vec<String> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let lane_index = lanes
            .iter()
            .position(|oid| oid == &commit.id)
            .unwrap_or_else(|| {
                lanes.insert(0, commit.id.clone());
                0
            });

        let mut graph = String::new();
        for (index, _) in lanes.iter().enumerate() {
            graph.push(if index == lane_index { '*' } else { '|' });
            if index + 1 != lanes.len() {
                graph.push(' ');
            }
        }

        rows.push(GraphLine::Commit {
            graph,
            summary: commit.summary.clone(),
        });

        lanes.remove(lane_index);
        let mut insert_at = lane_index.min(lanes.len());
        for parent in &commit.parents {
            if lanes.iter().any(|lane| lane == parent) {
                continue;
            }
            lanes.insert(insert_at, parent.clone());
            insert_at = insert_at.saturating_add(1);
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::{parse_history_records, render_graph, HistoryCommit};
    use crate::domain::{CommitSummary, GraphLine};

    fn commit(id: &str, parents: &[&str], subject: &str) -> HistoryCommit {
        HistoryCommit {
            id: id.to_string(),
            summary: CommitSummary {
                hash: id.to_string(),
                author: "A".to_string(),
                date: "2026-08-10".to_string(),
                subject: subject.to_string(),
            },
            parents: parents.iter().map(|parent| (*parent).to_string()).collect(),
        }
    }

    #[test]
    fn parses_nul_delimited_history_records() {
        let output = b"222\x00111\x00Alice\x002026-08-10\x00second\x00111\x00\x00Alice\x002026-08-09\x00first\x00";
        let commits = parse_history_records(output).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "222");
        assert_eq!(commits[0].parents, vec!["111"]);
        assert_eq!(commits[1].parents, Vec::<String>::new());
    }

    #[test]
    fn parser_rejects_incomplete_records() {
        let error = parse_history_records(b"only\0three\0fields\0").unwrap_err();
        assert!(error.to_string().contains("record shape"));
    }

    #[test]
    fn graph_collapses_duplicate_parent_lanes_after_merge() {
        let commits = vec![
            commit("merge", &["left", "right"], "merge"),
            commit("left", &["base"], "left"),
            commit("right", &["base"], "right"),
            commit("base", &[], "base"),
        ];

        let graph = render_graph(&commits);
        let rows = graph
            .iter()
            .map(|row| match row {
                GraphLine::Commit { graph, .. } => graph.as_str(),
                GraphLine::Connector { graph } => graph.as_str(),
            })
            .collect::<Vec<_>>();

        assert_eq!(rows, vec!["*", "* |", "| *", "*"]);
    }
}
