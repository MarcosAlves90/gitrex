use std::path::{Path, PathBuf};

use git2::{
    build::CheckoutBuilder, BranchType, Cred, CredentialType, ErrorCode, FetchOptions, FetchPrune,
    Oid, PushOptions, RemoteCallbacks, Repository,
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

    pub(crate) fn repo(&self) -> Result<Repository> {
        Repository::discover(&self.discovery_path).map_err(map_repo_error)
    }

    pub(crate) fn git(&self) -> super::GitProcess {
        super::GitProcess::new(&self.discovery_path)
    }

    pub fn status(&self) -> Result<crate::domain::RepoStatus> {
        crate::git::status::read_status(self)
    }

    pub fn refresh_remote_refs(&self) -> Result<()> {
        self.fetch(None)
    }

    pub fn fetch(&self, remote_name: Option<&str>) -> Result<()> {
        let repo = self.repo()?;
        if let Some(remote_name) = remote_name {
            return fetch_remote(&repo, remote_name);
        }

        let remotes = repo.remotes().map_err(map_git_error)?;
        for remote_name in remotes.iter().flatten() {
            fetch_remote(&repo, remote_name)?;
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
        let repo = self.repo()?;

        if let Ok(branch) = repo.find_branch(target, BranchType::Local) {
            let commit = branch.get().peel_to_commit().map_err(map_git_error)?;
            ensure_checkout_safe(&repo, commit.as_object())?;
            repo.set_head(&format!("refs/heads/{target}"))
                .map_err(map_git_error)?;
            checkout_head_safely(&repo)?;
            return Ok(());
        }

        let obj = repo.revparse_single(target).map_err(map_git_error)?;
        let commit = obj.peel_to_commit().map_err(map_git_error)?;
        ensure_checkout_safe(&repo, commit.as_object())?;
        repo.set_head_detached(commit.id()).map_err(map_git_error)?;
        checkout_head_safely(&repo)?;
        Ok(())
    }

    pub fn switch(&self, target: &str) -> Result<()> {
        let repo = self.repo()?;
        let branch = repo
            .find_branch(target, BranchType::Local)
            .map_err(map_git_error)?;
        let commit = branch.get().peel_to_commit().map_err(map_git_error)?;
        ensure_checkout_safe(&repo, commit.as_object())?;
        repo.set_head(&format!("refs/heads/{target}"))
            .map_err(map_git_error)?;
        checkout_head_safely(&repo)
    }

    pub fn create_branch(&self, branch: &str, start_point: Option<&str>) -> Result<()> {
        let repo = self.repo()?;
        let commit_oid = match start_point {
            Some(reference) => resolve_commit_oid(&repo, reference)?,
            None => repo
                .head()
                .map_err(map_git_error)?
                .peel_to_commit()
                .map_err(map_git_error)?
                .id(),
        };
        let commit = repo.find_commit(commit_oid).map_err(map_git_error)?;
        repo.branch(branch, &commit, false).map_err(map_git_error)?;
        self.switch(branch)
    }

    pub fn delete_local_branch(&self, branch: &str) -> Result<()> {
        self.git().ensure_repository()?;
        self.git().run(["branch", "-d", "--", branch])?;
        Ok(())
    }

    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<()> {
        let repo = self.repo()?;
        let mut remote = repo.find_remote(remote).map_err(map_git_error)?;

        let mut options = PushOptions::new();
        options.remote_callbacks(remote_callbacks(&repo)?);
        let refspec = format!(":refs/heads/{branch}");
        remote
            .push(&[refspec.as_str()], Some(&mut options))
            .map_err(map_git_error)?;
        Ok(())
    }

    pub fn clone_repository(&self, repository: &str, directory: Option<&Path>) -> Result<()> {
        let path = match directory {
            Some(path) => path.to_path_buf(),
            None => default_clone_path(repository),
        };
        Repository::clone(repository, &path).map_err(map_git_error)?;
        Ok(())
    }

    pub fn pull(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let repo = self.repo()?;
        let branch_name = branch
            .map(ToOwned::to_owned)
            .or_else(|| {
                repo.head()
                    .ok()
                    .and_then(|head| head.shorthand().map(ToOwned::to_owned))
            })
            .ok_or(GitError::NotRepository)?;
        let remote_name = remote.unwrap_or("origin");

        let mut remote = repo.find_remote(remote_name).map_err(map_git_error)?;
        let mut options = FetchOptions::new();
        options.remote_callbacks(remote_callbacks(&repo)?);
        remote
            .fetch(&[branch_name.as_str()], Some(&mut options), None)
            .map_err(map_git_error)?;

        let remote_ref_name = format!("refs/remotes/{remote_name}/{branch_name}");
        let remote_commit = resolve_commit_oid(&repo, &remote_ref_name)?;

        let mut local_ref = repo
            .find_reference(&format!("refs/heads/{branch_name}"))
            .map_err(map_git_error)?;
        let local_commit = local_ref.peel_to_commit().map_err(map_git_error)?;
        let (ahead, behind) = repo
            .graph_ahead_behind(local_commit.id(), remote_commit)
            .map_err(map_git_error)?;

        if behind == 0 {
            return Ok(());
        }

        if ahead > 0 {
            return Err(GitError::Backend(String::from(
                "pull cannot fast-forward because local and remote histories have diverged",
            )));
        }

        let remote_target = repo.find_commit(remote_commit).map_err(map_git_error)?;
        ensure_checkout_safe(&repo, remote_target.as_object())?;
        local_ref
            .set_target(remote_commit, "fast-forward")
            .map_err(map_git_error)?;
        repo.set_head(&format!("refs/heads/{branch_name}"))
            .map_err(map_git_error)?;
        checkout_head_safely(&repo)
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let repo = self.repo()?;
        let branch_name = branch
            .map(ToOwned::to_owned)
            .or_else(|| {
                repo.head()
                    .ok()
                    .and_then(|head| head.shorthand().map(ToOwned::to_owned))
            })
            .ok_or(GitError::NotRepository)?;
        let remote_name = remote.unwrap_or("origin");
        let mut remote = repo.find_remote(remote_name).map_err(map_git_error)?;

        let mut options = PushOptions::new();
        options.remote_callbacks(remote_callbacks(&repo)?);
        remote
            .push(
                &[format!("refs/heads/{branch_name}:refs/heads/{branch_name}")],
                Some(&mut options),
            )
            .map_err(map_git_error)?;
        Ok(())
    }
}

fn fetch_remote(repo: &Repository, remote_name: &str) -> Result<()> {
    let mut remote = repo.find_remote(remote_name).map_err(map_git_error)?;
    let mut options = FetchOptions::new();
    options.prune(FetchPrune::On);
    options.remote_callbacks(remote_callbacks(repo)?);
    remote
        .fetch(&[] as &[&str], Some(&mut options), None)
        .map_err(map_git_error)
}

fn ensure_checkout_safe(repo: &Repository, target: &git2::Object<'_>) -> Result<()> {
    let mut builder = CheckoutBuilder::new();
    builder.safe().dry_run();
    repo.checkout_tree(target, Some(&mut builder))
        .map_err(map_git_error)
}

fn checkout_head_safely(repo: &Repository) -> Result<()> {
    let mut builder = CheckoutBuilder::new();
    builder.safe();
    repo.checkout_head(Some(&mut builder))
        .map_err(map_git_error)
}

fn remote_callbacks(repo: &Repository) -> Result<RemoteCallbacks<'static>> {
    let config = repo.config().map_err(map_git_error)?;
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, username_from_url, allowed_types| {
        resolve_credentials(&config, url, username_from_url, allowed_types)
    });
    Ok(callbacks)
}

fn resolve_credentials(
    config: &git2::Config,
    url: &str,
    username_from_url: Option<&str>,
    allowed_types: CredentialType,
) -> std::result::Result<Cred, git2::Error> {
    let username = username_from_url.unwrap_or("git");

    if allowed_types.is_ssh_key() || url.starts_with("ssh://") || url.starts_with("git@") {
        if let Ok(cred) = Cred::ssh_key_from_agent(username) {
            return Ok(cred);
        }
    }

    if allowed_types.is_user_pass_plaintext()
        || allowed_types.is_username()
        || allowed_types.is_default()
    {
        if let Ok(cred) = Cred::credential_helper(config, url, username_from_url) {
            return Ok(cred);
        }
    }

    if allowed_types.is_username() {
        if let Ok(cred) = Cred::username(username) {
            return Ok(cred);
        }
    }

    if allowed_types.is_default() {
        return Cred::default();
    }

    Err(git2::Error::from_str(
        "unable to resolve local git credentials",
    ))
}

fn resolve_commit_oid(repo: &Repository, reference: &str) -> Result<Oid> {
    let object = repo.revparse_single(reference).map_err(map_git_error)?;
    object
        .peel_to_commit()
        .map_err(map_git_error)
        .map(|commit| commit.id())
}

fn default_clone_path(repository: &str) -> PathBuf {
    let trimmed = repository.trim_end_matches('/');
    let name = trimmed
        .rsplit('/')
        .next()
        .unwrap_or("repository")
        .trim_end_matches(".git");
    PathBuf::from(name)
}

fn map_repo_error(error: git2::Error) -> GitError {
    if error.code() == ErrorCode::NotFound {
        GitError::NotRepository
    } else {
        GitError::Backend(error.message().to_string())
    }
}

fn map_git_error(error: git2::Error) -> GitError {
    map_repo_error(error)
}

#[cfg(test)]
mod tests {
    use super::GitClient;
    use crate::test_support::{
        checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
        current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
        CurrentDirGuard,
    };

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
