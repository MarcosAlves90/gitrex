use std::{
    path::{Path, PathBuf},
};

use git2::{
    build::CheckoutBuilder, BranchType, ErrorCode, ObjectType, Oid, PushOptions, RemoteCallbacks,
    Repository,
};

use crate::domain::error::{GitError, Result};

#[derive(Debug, Clone, Default)]
pub struct GitClient;

impl GitClient {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn repo(&self) -> Result<Repository> {
        Repository::discover(".").map_err(map_repo_error)
    }

    pub fn status(&self) -> Result<crate::domain::RepoStatus> {
        crate::git::status::read_status(self)
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

    pub fn checkout(&self, target: &str) -> Result<()> {
        let repo = self.repo()?;
        let mut builder = CheckoutBuilder::new();
        builder.force();

        if repo.find_branch(target, BranchType::Local).is_ok() {
            repo.set_head(&format!("refs/heads/{target}"))
                .map_err(map_git_error)?;
            repo.checkout_head(Some(&mut builder))
                .map_err(map_git_error)?;
            return Ok(());
        }

        let obj = repo.revparse_single(target).map_err(map_git_error)?;
        if let Ok(commit) = obj.peel_to_commit() {
            repo.set_head_detached(commit.id()).map_err(map_git_error)?;
            repo.checkout_head(Some(&mut builder))
                .map_err(map_git_error)?;
            return Ok(());
        }

        let tree = obj.peel(ObjectType::Tree).map_err(map_git_error)?;
        repo.checkout_tree(&tree, Some(&mut builder))
            .map_err(map_git_error)?;
        repo.set_head_detached(obj.id()).map_err(map_git_error)?;
        Ok(())
    }

    pub fn switch(&self, target: &str) -> Result<()> {
        let repo = self.repo()?;
        let mut builder = CheckoutBuilder::new();
        builder.force();
        repo.set_head(&format!("refs/heads/{target}"))
            .map_err(map_git_error)?;
        repo.checkout_head(Some(&mut builder))
            .map_err(map_git_error)?;
        Ok(())
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

    pub fn clone(&self, repository: &str, directory: Option<&Path>) -> Result<()> {
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
            .or_else(|| repo.head().ok().and_then(|head| head.shorthand().map(ToOwned::to_owned)))
            .ok_or(GitError::NotRepository)?;
        let remote_name = remote.unwrap_or("origin");

        let mut remote = repo.find_remote(remote_name).map_err(map_git_error)?;
        remote
            .fetch(&[branch_name.as_str()], None, None)
            .map_err(map_git_error)?;

        let remote_ref_name = format!("refs/remotes/{remote_name}/{branch_name}");
        let remote_commit = resolve_commit_oid(&repo, &remote_ref_name)?;

        let mut local_ref = repo
            .find_reference(&format!("refs/heads/{branch_name}"))
            .map_err(map_git_error)?;
        let local_commit = local_ref.peel_to_commit().map_err(map_git_error)?;

        if repo
            .graph_descendant_of(remote_commit, local_commit.id())
            .map_err(map_git_error)?
        {
            local_ref
                .set_target(remote_commit, "fast-forward")
                .map_err(map_git_error)?;
            let mut builder = CheckoutBuilder::new();
            builder.force();
            repo.set_head(&format!("refs/heads/{branch_name}"))
                .map_err(map_git_error)?;
            repo.checkout_head(Some(&mut builder))
                .map_err(map_git_error)?;
            return Ok(());
        }

        Err(GitError::Backend(String::from(
            "pull requires a fast-forward update",
        )))
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let repo = self.repo()?;
        let branch_name = branch
            .map(ToOwned::to_owned)
            .or_else(|| repo.head().ok().and_then(|head| head.shorthand().map(ToOwned::to_owned)))
            .ok_or(GitError::NotRepository)?;
        let remote_name = remote.unwrap_or("origin");
        let mut remote = repo.find_remote(remote_name).map_err(map_git_error)?;

        let mut options = PushOptions::new();
        let callbacks = RemoteCallbacks::new();
        options.remote_callbacks(callbacks);
        remote
            .push(
                &[format!("refs/heads/{branch_name}:refs/heads/{branch_name}")],
                Some(&mut options),
            )
            .map_err(map_git_error)?;
        Ok(())
    }
}

fn resolve_commit_oid(repo: &Repository, reference: &str) -> Result<Oid> {
    let object = repo.revparse_single(reference).map_err(map_git_error)?;
    object.peel_to_commit().map_err(map_git_error).map(|commit| commit.id())
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
