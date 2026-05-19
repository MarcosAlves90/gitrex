use std::path::Path;
use std::process::Command;

use crate::domain::error::{GitError, Result};

#[derive(Debug, Clone, Default)]
pub struct GitClient;

impl GitClient {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, args: &[String]) -> Result<String> {
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("not a git repository") {
                return Err(GitError::NotRepository);
            }
            return Err(GitError::CommandFailed {
                code: output.status.code(),
                stderr,
            });
        }

        String::from_utf8(output.stdout)
            .map(|text| text.trim_end().to_string())
            .map_err(|_| GitError::Utf8)
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

    pub fn checkout(&self, target: &str) -> Result<()> {
        self.run(&["checkout".to_string(), target.to_string()]).map(|_| ())
    }

    pub fn switch(&self, target: &str) -> Result<()> {
        self.run(&["switch".to_string(), target.to_string()]).map(|_| ())
    }

    pub fn clone(&self, repository: &str, directory: Option<&Path>) -> Result<()> {
        let mut args = vec!["clone".to_string(), repository.to_string()];
        if let Some(directory) = directory {
            args.push(directory.display().to_string());
        }
        self.run(&args).map(|_| ())
    }

    pub fn pull(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let mut args = vec!["pull".to_string()];
        if let Some(remote) = remote {
            args.push(remote.to_string());
        }
        if let Some(branch) = branch {
            args.push(branch.to_string());
        }
        self.run(&args).map(|_| ())
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let mut args = vec!["push".to_string()];
        if let Some(remote) = remote {
            args.push(remote.to_string());
        }
        if let Some(branch) = branch {
            args.push(branch.to_string());
        }
        self.run(&args).map(|_| ())
    }

    pub(crate) fn run_git(&self, args: &[String]) -> Result<String> {
        self.run(args)
    }
}
