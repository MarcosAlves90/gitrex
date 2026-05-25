use chrono::{TimeZone, Utc};
use git2::{Oid, Repository, Sort};

use crate::domain::{BranchHistory, CommitSummary, GraphLine};

use super::GitClient;

pub fn read_branch_history(client: &GitClient, reference: &str) -> crate::domain::Result<BranchHistory> {
    let repo = client.repo()?;
    let start = resolve_reference(&repo, reference)?;
    let commits = collect_commits(&repo, start)?;
    let graph = render_graph(&commits);
    Ok(BranchHistory {
        commits: commits.into_iter().map(|commit| commit.summary).collect(),
        graph,
    })
}

#[derive(Debug, Clone)]
struct HistoryCommit {
    id: Oid,
    summary: CommitSummary,
    parents: Vec<Oid>,
}

fn resolve_reference(repo: &Repository, reference: &str) -> crate::domain::Result<Oid> {
    let candidates = if reference.starts_with("refs/") {
        vec![reference.to_string()]
    } else {
        vec![format!("refs/heads/{reference}"), reference.to_string()]
    };

    for candidate in candidates {
        if let Ok(object) = repo.revparse_single(&candidate) {
            if let Ok(commit) = object.peel_to_commit() {
                return Ok(commit.id());
            }
        }
    }

    Err(crate::domain::GitError::Backend(format!(
        "unknown reference: {reference}"
    )))
}

fn collect_commits(repo: &Repository, start: Oid) -> crate::domain::Result<Vec<HistoryCommit>> {
    let mut revwalk = repo.revwalk().map_err(map_error)?;
    revwalk.push(start).map_err(map_error)?;
    revwalk
        .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(map_error)?;

    let mut commits = Vec::new();
    for oid in revwalk {
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

fn render_graph(commits: &[HistoryCommit]) -> Vec<GraphLine> {
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

#[cfg(test)]
mod tests {
    use super::read_branch_history;
    use crate::git::GitClient;
    use crate::test_support::{
        checkout_branch, commit_all, configure_user, create_branch, current_dir_lock, init_repo,
        write_file, CurrentDirGuard,
    };

    #[test]
    fn reads_only_requested_branch_history() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base commit");
        create_branch(&repo, "feature/login", "HEAD");
        checkout_branch(&repo, "feature/login");
        write_file(temp.path(), "README.md", "feature work\n");
        commit_all(&repo, "feature work");
        checkout_branch(&repo, "main");
        write_file(temp.path(), "README.md", "main work\n");
        commit_all(&repo, "main work");

        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        let history = read_branch_history(&client, "main").unwrap();

        assert!(history.commits.iter().any(|entry| entry.subject == "main work"));
        assert!(!history.commits.iter().any(|entry| entry.subject == "feature work"));
    }
}
