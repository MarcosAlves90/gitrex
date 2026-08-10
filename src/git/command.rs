use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::domain::error::{GitError, Result};

#[derive(Debug, Clone)]
pub struct GitClient {
    discovery_path: PathBuf,
}

impl Default for GitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClient {
    pub fn new() -> Self {
        let discovery_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self { discovery_path }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let discovery_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };

        Self { discovery_path }
    }

    pub fn discovery_path(&self) -> &Path {
        &self.discovery_path
    }

    pub(crate) fn git(&self) -> super::GitProcess {
        super::GitProcess::new(&self.discovery_path)
    }

    pub(crate) fn resolve_commit(&self, reference: &str) -> Result<String> {
        let git = self.git();
        git.ensure_repository()?;
        let candidate = format!("{reference}^{{commit}}");
        let output = git.probe([
            "rev-parse",
            "--verify",
            "--end-of-options",
            candidate.as_str(),
        ])?;
        if !output.success() {
            return Err(GitError::ReferenceNotFound(reference.to_string()));
        }

        let oid = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
        let oid = oid.trim();
        if oid.is_empty() {
            return Err(GitError::Parse(format!(
                "empty object id for reference {reference}"
            )));
        }
        Ok(oid.to_string())
    }

    pub fn status(&self) -> Result<crate::domain::RepoStatus> {
        crate::git::status::read_status(self)
    }

    pub fn refresh_remote_refs(&self) -> Result<()> {
        self.fetch(None)
    }

    pub fn fetch(&self, remote_name: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        match remote_name {
            Some(remote_name) => {
                git.run(["fetch", "--prune", "--", remote_name])?;
            }
            None => {
                git.run(["fetch", "--all", "--prune"])?;
            }
        }
        Ok(())
    }

    pub fn branches(&self) -> Result<Vec<crate::domain::BranchInfo>> {
        crate::git::branch::list_branches(self)
    }

    pub fn log(&self, limit: usize) -> Result<Vec<crate::domain::CommitSummary>> {
        crate::git::log::read_log(self, limit)
    }

    pub fn history_for_ref(&self, reference: &str) -> Result<crate::domain::BranchHistory> {
        crate::git::read_branch_history(self, reference)
    }

    pub fn snapshot(&self) -> Result<crate::domain::RepoSnapshot> {
        crate::git::read_snapshot(self)
    }

    pub fn checkout(&self, target: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let local_ref = format!("refs/heads/{target}");
        let local_branch = git.probe(["show-ref", "--verify", "--quiet", local_ref.as_str()])?;

        if local_branch.success() {
            git.run(["switch", "--", target])?;
            return Ok(());
        }

        self.resolve_commit(target)?;
        git.run(["switch", "--detach", "--", target])?;
        Ok(())
    }

    pub fn switch(&self, target: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let local_ref = format!("refs/heads/{target}");
        let local_branch = git.probe(["show-ref", "--verify", "--quiet", local_ref.as_str()])?;
        if !local_branch.success() {
            return Err(GitError::ReferenceNotFound(target.to_string()));
        }
        git.run(["switch", "--", target])?;
        Ok(())
    }

    pub fn create_branch(&self, branch: &str, start_point: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let mut args = vec![
            OsString::from("switch"),
            OsString::from("-c"),
            OsString::from(branch),
        ];
        if let Some(start_point) = start_point {
            args.push(OsString::from("--"));
            args.push(OsString::from(start_point));
        }
        git.run(args)?;
        Ok(())
    }

    pub fn delete_local_branch(&self, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        git.run(["branch", "-d", "--", branch])?;
        Ok(())
    }

    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        git.run(["push", "--delete", "--", remote, branch])?;
        Ok(())
    }

    pub fn clone_repository(&self, repository: &str, directory: Option<&Path>) -> Result<()> {
        let git = self.git();
        let path = match directory {
            Some(path) => path.to_path_buf(),
            None => default_clone_path(repository),
        };
        let args = vec![
            OsString::from("clone"),
            OsString::from("--"),
            OsString::from(repository),
            path.as_os_str().to_os_string(),
        ];
        git.run(args)?;
        Ok(())
    }

    pub fn pull(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;

        match (remote, branch) {
            (None, None) => {
                git.run(["pull", "--ff-only"])?;
            }
            (Some(remote), None) => {
                git.run(["pull", "--ff-only", "--", remote])?;
            }
            (remote, Some(branch)) => {
                let remote = remote.unwrap_or("origin");
                git.run(["fetch", "--prune", "--", remote, branch])?;
                let (ahead, behind) = ahead_behind(&git, "HEAD", "FETCH_HEAD")?;
                if behind == 0 {
                    return Ok(());
                }
                if ahead > 0 {
                    return Err(GitError::Diverged { ahead, behind });
                }
                git.run(["merge", "--ff-only", "FETCH_HEAD"])?;
            }
        }
        Ok(())
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;

        match (remote, branch) {
            (None, None) => {
                git.run(["push"])?;
            }
            (Some(remote), None) => {
                git.run(["push", "--", remote])?;
            }
            (remote, Some(branch)) => {
                let remote = remote.unwrap_or("origin");
                let refspec = format!("HEAD:refs/heads/{branch}");
                git.run(["push", "--", remote, refspec.as_str()])?;
            }
        }
        Ok(())
    }
}

fn ahead_behind(git: &super::GitProcess, left: &str, right: &str) -> Result<(u32, u32)> {
    let range = format!("{left}...{right}");
    let output = git.run_text(["rev-list", "--left-right", "--count", range.as_str()])?;
    let mut counts = output.split_whitespace();
    let ahead = counts
        .next()
        .ok_or_else(|| GitError::Parse(String::from("missing ahead count")))?
        .parse::<u32>()
        .map_err(|_| GitError::Parse(String::from("invalid ahead count")))?;
    let behind = counts
        .next()
        .ok_or_else(|| GitError::Parse(String::from("missing behind count")))?
        .parse::<u32>()
        .map_err(|_| GitError::Parse(String::from("invalid behind count")))?;
    Ok((ahead, behind))
}

fn default_clone_path(repository: &str) -> PathBuf {
    let trimmed = repository.trim_end_matches('/');
    let name = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repository")
        .trim_end_matches(".git");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::{default_clone_path, GitClient};
    use crate::test_support::{
        checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
        current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
        CurrentDirGuard,
    };

    #[test]
    fn default_clone_path_handles_https_and_scp_style_urls() {
        assert_eq!(
            default_clone_path("https://example.com/acme/project.git"),
            std::path::PathBuf::from("project")
        );
        assert_eq!(
            default_clone_path("git@example.com:acme/project.git"),
            std::path::PathBuf::from("project")
        );
    }

    #[test]
    fn delete_local_branch_removes_ref_from_repository() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "initial commit");
        create_branch(&repo, "feature/login", "HEAD");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        client.delete_local_branch("feature/login").unwrap();

        assert!(repo
            .find_branch("feature/login", git2::BranchType::Local)
            .is_err());
    }

    #[test]
    fn delete_local_branch_refuses_unmerged_commits() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "initial commit");
        create_branch(&repo, "feature/unique", "HEAD");
        checkout_branch(&repo, "feature/unique");
        write_file(temp.path(), "feature.txt", "unique\n");
        commit_all(&repo, "unique feature commit");
        checkout_branch(&repo, "main");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        assert!(client.delete_local_branch("feature/unique").is_err());

        assert!(repo
            .find_branch("feature/unique", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn delete_remote_branch_pushes_refspec_to_remote() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let seed = temp.path().join("seed");
        let origin = temp.path().join("origin.git");
        let worktree = temp.path().join("worktree");

        let seed_repo = init_repo(&seed, "main");
        configure_user(&seed_repo);
        write_file(&seed, "README.md", "base\n");
        commit_all(&seed_repo, "initial commit");

        let origin_repo = clone_bare_repo(&seed, &origin);
        set_remote_head(&origin_repo, "refs/heads/main");

        let worktree_repo = clone_repo(&origin, &worktree);
        configure_user(&worktree_repo);
        set_upstream(&worktree_repo, "main", "origin/main");

        write_file(&worktree, "feature.txt", "feature\n");
        commit_all(&worktree_repo, "feature work");
        create_branch(&worktree_repo, "feature/login", "HEAD");
        checkout_branch(&worktree_repo, "feature/login");
        push_branch(&worktree_repo, "origin", "feature/login");

        let _restore = CurrentDirGuard::push(&worktree);
        let client = GitClient::new();
        client
            .delete_remote_branch("origin", "feature/login")
            .unwrap();

        let origin_repo = git2::Repository::open_bare(&origin).unwrap();
        assert!(origin_repo
            .find_reference("refs/heads/feature/login")
            .is_err());
    }
}
