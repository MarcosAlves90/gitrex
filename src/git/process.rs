use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

use crate::domain::{GitError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitOutput {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl GitOutput {
    pub(crate) fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GitProcess {
    repository: PathBuf,
}

impl GitProcess {
    pub(crate) fn new(repository: impl AsRef<Path>) -> Self {
        Self {
            repository: repository.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn ensure_repository(&self) -> Result<()> {
        let output = self.probe(["rev-parse", "--git-dir"])?;
        if output.success() {
            Ok(())
        } else {
            Err(GitError::NotRepository)
        }
    }

    pub(crate) fn probe<I, S>(&self, args: I) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = args
            .into_iter()
            .map(|value| value.as_ref().to_os_string())
            .collect::<Vec<OsString>>();
        self.output_args(&args)
    }

    pub(crate) fn run<I, S>(&self, args: I) -> Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = args
            .into_iter()
            .map(|value| value.as_ref().to_os_string())
            .collect::<Vec<OsString>>();
        let command = args
            .first()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("git"));
        let output = self.output_args(&args)?;
        if output.success() {
            Ok(output)
        } else {
            Err(GitError::CommandFailed {
                command,
                exit_code: output.exit_code,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    pub(crate) fn run_text<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run(args)?;
        String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)
    }

    fn output_args(&self, args: &[OsString]) -> Result<GitOutput> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    GitError::GitNotInstalled
                } else {
                    GitError::Io(error)
                }
            })?;

        Ok(GitOutput {
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GitProcess;
    use crate::domain::GitError;

    #[test]
    fn process_detects_repository_and_reports_command_failure() {
        let temp = tempfile::TempDir::new().unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(init.status.success());

        let git = GitProcess::new(temp.path());
        git.ensure_repository().unwrap();
        let output = git.run(["rev-parse", "--git-dir"]).unwrap();
        assert!(output.success());

        let missing = git
            .probe(["show-ref", "--verify", "--quiet", "refs/heads/missing"])
            .unwrap();
        assert!(!missing.success());

        let error = git
            .run(["rev-parse", "--verify", "missing-ref"])
            .unwrap_err();
        assert!(matches!(error, GitError::CommandFailed { .. }));
    }

    #[test]
    fn process_maps_non_repository_without_parsing_stderr() {
        let temp = tempfile::TempDir::new().unwrap();
        let git = GitProcess::new(temp.path());
        assert!(matches!(
            git.ensure_repository(),
            Err(GitError::NotRepository)
        ));
    }
}
