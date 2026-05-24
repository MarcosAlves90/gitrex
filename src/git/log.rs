use crate::domain::{CommitSummary, GraphLine};

use super::GitClient;

pub fn read_log(client: &GitClient, limit: usize) -> crate::domain::Result<Vec<CommitSummary>> {
    let output = client.run_git(&[
        "log".to_string(),
        format!("-n{limit}"),
        "--date=short".to_string(),
        "--pretty=format:%H%x09%an%x09%ad%x09%s".to_string(),
    ])?;
    Ok(parse_log_lines(&output))
}

pub fn parse_log_lines(output: &str) -> Vec<CommitSummary> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            Some(CommitSummary {
                hash: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                subject: parts.next()?.to_string(),
            })
        })
        .collect()
}

pub fn parse_graph_log_lines(output: &str) -> Vec<GraphLine> {
    output
        .lines()
        .filter_map(|line| {
            let Some((graph, rest)) = line.split_once('\t') else {
                return Some(GraphLine::Connector {
                    graph: line.to_string(),
                });
            };

            let mut parts = rest.splitn(4, '\t');
            Some(GraphLine::Commit {
                graph: graph.to_string(),
                summary: CommitSummary {
                    hash: parts.next()?.to_string(),
                    author: parts.next()?.to_string(),
                    date: parts.next()?.to_string(),
                    subject: parts.next()?.to_string(),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_graph_log_lines, parse_log_lines};

    #[test]
    fn parses_log_rows() {
        let sample = "abc123\tMarcos\t2026-05-18\tInitial commit\n";
        let entries = parse_log_lines(sample);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].subject, "Initial commit");
    }

    #[test]
    fn parses_graph_rows() {
        let sample = "*\tabc123\tMarcos\t2026-05-18\tInitial commit\n|/  \n";
        let entries = parse_graph_log_lines(sample);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], crate::domain::GraphLine::Commit { .. }));
        assert!(matches!(entries[1], crate::domain::GraphLine::Connector { .. }));
    }
}
