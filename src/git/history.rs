use std::ffi::OsString;

use crate::domain::BranchHistory;

use super::shared::{parse_history_records, render_graph};
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
    let git = client.git();
    git.ensure_repository()?;
    let start = client.resolve_commit(reference)?;
    let max_count = format!("--max-count={limit}");
    let args = vec![
        OsString::from("log"),
        OsString::from("-z"),
        OsString::from("--topo-order"),
        OsString::from(max_count),
        OsString::from("--date=format:%Y-%m-%d"),
        OsString::from("--format=%H%x00%P%x00%an%x00%ad%x00%s"),
        OsString::from(start),
    ];
    let output = git.run(args)?;
    let commits = parse_history_records(&output.stdout)?;
    let graph = render_graph(&commits);
    Ok(BranchHistory {
        commits: commits.into_iter().map(|commit| commit.summary).collect(),
        graph,
    })
}

#[cfg(test)]
mod tests {
    use super::{read_branch_history, read_branch_history_with_limit};
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
        assert!(history.commits.iter().all(|entry| entry.hash.len() == 40));
    }

    #[test]
    fn history_limit_is_enforced_by_system_git() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "one\n");
        commit_all(&repo, "one");
        write_file(temp.path(), "README.md", "two\n");
        commit_all(&repo, "two");
        write_file(temp.path(), "README.md", "three\n");
        commit_all(&repo, "three");

        let client = GitClient::from_path(temp.path());
        let history = read_branch_history_with_limit(&client, "main", 2).unwrap();

        assert_eq!(history.commits.len(), 2);
        assert_eq!(history.commits[0].subject, "three");
        assert_eq!(history.commits[1].subject, "two");
    }

    #[test]
    fn unknown_history_reference_is_typed() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base");

        let error = read_branch_history(&GitClient::from_path(temp.path()), "missing").unwrap_err();
        assert!(matches!(
            error,
            crate::domain::GitError::ReferenceNotFound(reference) if reference == "missing"
        ));
    }
}
