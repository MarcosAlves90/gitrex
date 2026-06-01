use chrono::{TimeZone, Utc};
use git2::{Oid, Repository, Sort};

use crate::domain::{CommitSummary, GitError, GraphLine};

pub fn map_error(error: git2::Error) -> GitError {
    if error.code() == git2::ErrorCode::NotFound {
        GitError::NotRepository
    } else {
        GitError::Backend(error.message().to_string())
    }
}

pub fn short_oid(oid: Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

pub fn short_date(seconds: i64) -> String {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| seconds.to_string())
}

pub struct HistoryCommit {
    pub id: Oid,
    pub summary: CommitSummary,
    pub parents: Vec<Oid>,
}

pub fn collect_history_commits(
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
        commits.push(HistoryCommit {
            id: commit.id(),
            summary: CommitSummary {
                hash: short_oid(commit.id()),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                date: short_date(commit.time().seconds()),
                subject: commit.summary().unwrap_or_default().to_string(),
            },
            parents: commit.parent_ids().collect(),
        });
    }
    Ok(commits)
}

pub fn render_graph(commits: &[HistoryCommit]) -> Vec<GraphLine> {
    let mut lanes: Vec<Oid> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let lane_index = lanes
            .iter()
            .position(|oid| *oid == commit.id)
            .unwrap_or_else(|| {
                lanes.insert(0, commit.id);
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

        if commit.parents.is_empty() {
            lanes.remove(lane_index);
            continue;
        }

        lanes[lane_index] = commit.parents[0];
        for parent in commit.parents.iter().skip(1).rev() {
            lanes.insert(lane_index + 1, *parent);
        }
    }

    rows
}
