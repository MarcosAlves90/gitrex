use chrono::{TimeZone, Utc};
use git2::{Oid, Repository, Sort};

use crate::domain::{CommitSummary, GraphLine};

use super::GitClient;

pub fn read_log(client: &GitClient, limit: usize) -> crate::domain::Result<Vec<CommitSummary>> {
    let repo = client.repo()?;
    let start = head_oid(&repo)?;
    let commits = collect_commits(&repo, start, Some(limit))?;
    Ok(commits.into_iter().map(|commit| commit.summary).collect())
}

fn head_oid(repo: &Repository) -> crate::domain::Result<Oid> {
    repo.head()
        .map_err(map_error)?
        .peel_to_commit()
        .map_err(map_error)
        .map(|commit| commit.id())
}

fn collect_commits(
    repo: &Repository,
    start: Oid,
    limit: Option<usize>,
) -> crate::domain::Result<Vec<HistoryCommit>> {
    let mut revwalk = repo.revwalk().map_err(map_error)?;
    revwalk.push(start).map_err(map_error)?;
    revwalk
        .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(map_error)?;

    let mut commits = Vec::new();
    for oid in revwalk.take(limit.unwrap_or(usize::MAX)) {
        let oid = oid.map_err(map_error)?;
        let commit = repo.find_commit(oid).map_err(map_error)?;
        commits.push(HistoryCommit::from_commit(&commit));
    }
    Ok(commits)
}

#[derive(Debug, Clone)]
struct HistoryCommit {
    summary: CommitSummary,
}

impl HistoryCommit {
    fn from_commit(commit: &git2::Commit<'_>) -> Self {
        Self {
            summary: CommitSummary {
                hash: short_oid(commit.id()),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                date: short_date(commit.time().seconds()),
                subject: commit.summary().unwrap_or_default().to_string(),
            },
        }
    }
}

fn short_oid(oid: Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

fn short_date(seconds: i64) -> String {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| seconds.to_string())
}

fn map_error(error: git2::Error) -> crate::domain::GitError {
    if error.code() == git2::ErrorCode::NotFound {
        crate::domain::GitError::NotRepository
    } else {
        crate::domain::GitError::Backend(error.message().to_string())
    }
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
        assert!(matches!(
            entries[0],
            crate::domain::GraphLine::Commit { .. }
        ));
        assert!(matches!(
            entries[1],
            crate::domain::GraphLine::Connector { .. }
        ));
    }
}
