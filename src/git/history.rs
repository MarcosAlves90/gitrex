use git2::{Oid, Repository};

use crate::domain::BranchHistory;

use super::shared::{collect_history_commits, render_graph};
use super::GitClient;

pub const DEFAULT_HISTORY_LIMIT: usize = 200;

pub fn read_branch_history(
    client: &GitClient,
    reference: &str,
) -> crate::domain::Result<BranchHistory> {
    read_branch_history_with_limit(client, reference, DEFAULT_HISTORY_LIMIT)
}

pub fn read_branch_history_with_limit(
    client: &GitClient,
    reference: &str,
    limit: usize,
) -> crate::domain::Result<BranchHistory> {
    let repo = client.repo()?;
    let start = resolve_reference(&repo, reference)?;
    let commits = collect_history_commits(&repo, start, Some(limit))?;
    let graph = render_graph(&commits);
    Ok(BranchHistory {
        commits: commits.into_iter().map(|commit| commit.summary).collect(),
        graph,
    })
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

        assert!(history
            .commits
            .iter()
            .any(|entry| entry.subject == "main work"));
        assert!(!history
            .commits
            .iter()
            .any(|entry| entry.subject == "feature work"));
    }
}
