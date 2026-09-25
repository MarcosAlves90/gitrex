use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedGitOutput {
    pub(crate) output: GitOutput,
    pub(crate) total_bytes: u64,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GitProcess {
    repository: PathBuf,
    read_only: bool,
}

impl GitProcess {
    pub(crate) fn new(repository: impl AsRef<Path>) -> Self {
        Self {
            repository: repository.as_ref().to_path_buf(),
            read_only: false,
        }
    }

    pub(crate) fn new_read_only(repository: impl AsRef<Path>) -> Self {
        Self {
            repository: repository.as_ref().to_path_buf(),
            read_only: true,
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

    pub(crate) fn run_with_input<I, S>(&self, args: I, input: &[u8]) -> Result<GitOutput>
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
        let mut child = self
            .command(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(map_spawn_error)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GitError::Backend("Git stdin pipe was unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError::Backend("Git stdout pipe was unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| GitError::Backend("Git stderr pipe was unavailable".to_string()))?;
        let stdout_reader = thread::spawn(move || {
            let mut output = Vec::new();
            let mut stdout = stdout;
            stdout.read_to_end(&mut output).map(|_| output)
        });
        let stderr_reader = thread::spawn(move || read_capped(stderr, 64 * 1024));
        let input_result = {
            let mut stdin = stdin;
            stdin.write_all(input)
        };
        let status = child.wait().map_err(GitError::Io)?;
        let stdout = stdout_reader
            .join()
            .map_err(|_| GitError::Backend("Git stdout reader stopped unexpectedly".to_string()))?
            .map_err(GitError::Io)?;
        let (stderr, stderr_truncated) = stderr_reader.join().map_err(|_| {
            GitError::Backend("Git stderr reader stopped unexpectedly".to_string())
        })??;
        input_result.map_err(GitError::Io)?;
        let stderr = if stderr_truncated {
            [stderr, b"\n[stderr truncated]".to_vec()].concat()
        } else {
            stderr
        };
        let output = GitOutput {
            exit_code: status.code(),
            stdout,
            stderr,
        };
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

    pub(crate) fn run_bounded<I, S>(
        &self,
        args: I,
        max_stdout_bytes: usize,
    ) -> Result<BoundedGitOutput>
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
        let mut child = self
            .command(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(map_spawn_error)?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError::Backend("Git stdout pipe was unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| GitError::Backend("Git stderr pipe was unavailable".to_string()))?;
        let stderr_reader = thread::spawn(move || read_capped(stderr, 64 * 1024));

        let mut captured_stdout = Vec::with_capacity(max_stdout_bytes.min(64 * 1024));
        let mut total_bytes = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            let read = stdout.read(&mut buffer).map_err(GitError::Io)?;
            if read == 0 {
                break;
            }
            total_bytes = total_bytes.saturating_add(read as u64);
            let remaining = max_stdout_bytes.saturating_sub(captured_stdout.len());
            captured_stdout.extend_from_slice(&buffer[..read.min(remaining)]);
        }

        let status = child.wait().map_err(GitError::Io)?;
        let (stderr, stderr_truncated) = stderr_reader.join().map_err(|_| {
            GitError::Backend("Git stderr reader stopped unexpectedly".to_string())
        })??;
        let stderr = if stderr_truncated {
            [stderr, b"\n[stderr truncated]".to_vec()].concat()
        } else {
            stderr
        };
        let output = GitOutput {
            exit_code: status.code(),
            stdout: captured_stdout,
            stderr,
        };
        if !output.success() {
            return Err(GitError::CommandFailed {
                command,
                exit_code: output.exit_code,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(BoundedGitOutput {
            output,
            total_bytes,
            truncated: total_bytes > max_stdout_bytes as u64,
        })
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
        let output = self.command(args).output().map_err(|error| {
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

    fn command(&self, args: &[OsString]) -> Command {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.repository);
        if self.read_only {
            command
                .arg("-c")
                .arg("core.fsmonitor=false")
                .env("GIT_OPTIONAL_LOCKS", "0")
                .env("GIT_NO_LAZY_FETCH", "1");
        }
        command.args(args).env("GIT_TERMINAL_PROMPT", "0");
        command
    }

    pub(crate) fn ensure_no_worktree_filters(&self) -> Result<()> {
        let output = self.probe([
            "config",
            "--null",
            "--get-regexp",
            r"^filter\..*\.(clean|process)$",
        ])?;

        if output.exit_code == Some(1) {
            return Ok(());
        }
        if !output.success() {
            return Err(GitError::CommandFailed {
                command: "config --null --get-regexp".to_string(),
                exit_code: output.exit_code,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let mut active_filters = HashSet::<Vec<u8>>::new();
        let mut records = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        if records.last() == Some(&&[][..]) {
            records.pop();
        }
        for record in records {
            let Some(separator) = record.iter().position(|byte| *byte == b'\n') else {
                return Err(GitError::Parse(
                    "malformed Git content filter configuration".to_string(),
                ));
            };
            let (key, value) = record.split_at(separator);
            let value = &value[1..];
            if value.is_empty() {
                continue;
            }
            let Some(driver) = key.strip_prefix(b"filter.").and_then(|key| {
                key.strip_suffix(b".clean")
                    .or_else(|| key.strip_suffix(b".process"))
            }) else {
                continue;
            };
            if !driver.is_empty() {
                active_filters.insert(driver.to_vec());
            }
        }
        if active_filters.is_empty() {
            return Ok(());
        }

        let tracked_paths = self.run(["ls-files", "-z"])?;
        if tracked_paths.stdout.is_empty() {
            return Ok(());
        }
        let attributes = self.run_with_input(
            ["check-attr", "-z", "--stdin", "filter"],
            &tracked_paths.stdout,
        )?;
        let mut fields = attributes
            .stdout
            .split(|byte| *byte == 0)
            .collect::<Vec<_>>();
        if fields.last() == Some(&&[][..]) {
            fields.pop();
        }
        if fields.len() % 3 != 0 {
            return Err(GitError::Parse(
                "malformed Git content filter attributes".to_string(),
            ));
        }
        let has_applicable_filter = fields.chunks_exact(3).any(|entry| {
            entry[1] == b"filter"
                && active_filters
                    .iter()
                    .any(|driver| driver.as_slice() == entry[2])
        });
        if has_applicable_filter {
            return Err(GitError::Backend(
                "worktree context is unavailable while Git clean or process filters are configured"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn read_capped(mut reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).map_err(GitError::Io)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    Ok((output, truncated))
}

fn map_spawn_error(error: std::io::Error) -> GitError {
    if error.kind() == std::io::ErrorKind::NotFound {
        GitError::GitNotInstalled
    } else {
        GitError::Io(error)
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

    #[test]
    fn read_only_worktree_guard_checks_filters_applied_to_tracked_paths() {
        let temp = tempfile::TempDir::new().unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(init.status.success());

        let tracked = temp.path().join("tracked.txt");
        std::fs::write(&tracked, "content\n").unwrap();
        let add = std::process::Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["add", "tracked.txt"])
            .output()
            .unwrap();
        assert!(add.status.success());

        let unused_filter = std::process::Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["config", "filter.unused.clean", "must-not-run"])
            .output()
            .unwrap();
        assert!(unused_filter.status.success());

        let git = GitProcess::new_read_only(temp.path());
        git.ensure_no_worktree_filters().unwrap();

        std::fs::write(
            temp.path().join(".gitattributes"),
            "tracked.txt filter=used\n",
        )
        .unwrap();
        let used_filter = std::process::Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["config", "filter.used.process", "must-not-run"])
            .output()
            .unwrap();
        assert!(used_filter.status.success());

        assert!(matches!(
            git.ensure_no_worktree_filters(),
            Err(GitError::Backend(_))
        ));
    }

    #[test]
    fn bounded_process_drains_output_but_keeps_only_the_requested_prefix() {
        let temp = tempfile::TempDir::new().unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(init.status.success());

        let git = GitProcess::new(temp.path());
        let output = git
            .run_bounded(["rev-parse", "--show-toplevel"], 3)
            .unwrap();
        assert_eq!(output.output.stdout.len(), 3);
        assert!(output.total_bytes > 3);
        assert!(output.truncated);
    }
}
