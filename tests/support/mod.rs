#![allow(dead_code)]

use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Mutex, OnceLock},
};

#[derive(Debug, Clone)]
pub struct TestRepo {
    path: PathBuf,
}

#[derive(Debug)]
pub struct TestGitError(String);

impl std::fmt::Display for TestGitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TestGitError {}

#[derive(Debug, Clone)]
pub struct TestHead {
    shorthand: Option<String>,
    target: Option<String>,
}

impl TestHead {
    pub fn shorthand(&self) -> Option<&str> {
        self.shorthand.as_deref()
    }

    pub fn target(&self) -> Option<String> {
        self.target.clone()
    }
}

pub struct TestReference {
    repo: TestRepo,
    name: String,
}

impl TestReference {
    pub fn target(&self) -> Option<String> {
        self.repo.reference_oid(&self.name)
    }

    pub fn delete(&mut self) -> Result<(), TestGitError> {
        self.repo.run(["update-ref", "-d", self.name.as_str()])?;
        Ok(())
    }
}

pub struct TestRemote {
    repo: TestRepo,
    name: String,
}

impl TestRemote {
    pub fn fetch(
        &mut self,
        branches: &[&str],
        _options: Option<()>,
        _reflog_message: Option<()>,
    ) -> Result<(), TestGitError> {
        let mut args = vec![
            "fetch".to_string(),
            "--quiet".to_string(),
            "--".to_string(),
            self.name.clone(),
        ];
        args.extend(branches.iter().map(|branch| (*branch).to_string()));
        self.repo.run(args.iter().map(String::as_str))?;
        Ok(())
    }
}

impl TestRepo {
    fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn head(&self) -> Result<TestHead, TestGitError> {
        let symbolic = self.probe(["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let shorthand = symbolic
            .status
            .success()
            .then(|| String::from_utf8_lossy(&symbolic.stdout).trim().to_string());

        let target = self.reference_oid("HEAD");

        Ok(TestHead { shorthand, target })
    }

    pub fn find_branch(&self, branch: &str) -> Result<(), TestGitError> {
        let reference = format!("refs/heads/{branch}");
        if self.reference_exists(&reference) {
            Ok(())
        } else {
            Err(TestGitError(format!("branch not found: {branch}")))
        }
    }

    pub fn find_reference(&self, reference: &str) -> Result<TestReference, TestGitError> {
        if self.reference_exists(reference) {
            Ok(TestReference {
                repo: self.clone(),
                name: reference.to_string(),
            })
        } else {
            Err(TestGitError(format!("reference not found: {reference}")))
        }
    }

    pub fn reference(
        &self,
        reference: &str,
        oid: String,
        _force: bool,
        _message: &str,
    ) -> Result<TestReference, TestGitError> {
        self.run(["update-ref", reference, oid.as_str()])?;
        self.find_reference(reference)
    }

    pub fn remote(&self, name: &str, url: &str) -> Result<TestRemote, TestGitError> {
        self.run(["remote", "add", "--", name, url])?;
        Ok(TestRemote {
            repo: self.clone(),
            name: name.to_string(),
        })
    }

    pub fn find_remote(&self, name: &str) -> Result<TestRemote, TestGitError> {
        let output = self.probe(["remote", "get-url", "--", name])?;
        if !output.status.success() {
            return Err(TestGitError(format!("remote not found: {name}")));
        }

        Ok(TestRemote {
            repo: self.clone(),
            name: name.to_string(),
        })
    }

    pub fn reference_exists(&self, reference: &str) -> bool {
        self.probe(["show-ref", "--verify", "--quiet", reference])
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn reference_oid(&self, reference: &str) -> Option<String> {
        let output = self
            .probe(["rev-parse", "--verify", "--end-of-options", reference])
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn run<I, S>(&self, args: I) -> Result<Output, TestGitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        checked_git(Some(&self.path), args)
    }

    fn probe<I, S>(&self, args: I) -> Result<Output, TestGitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        git_output(Some(&self.path), args)
    }
}

pub fn current_dir_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub fn push(path: &Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

pub fn write_file(path: &Path, name: &str, contents: &str) {
    fs::write(PathBuf::from(path).join(name), contents).unwrap();
}

pub fn init_repo(path: &Path, initial_head: &str) -> TestRepo {
    checked_git(
        None,
        [
            OsStr::new("init"),
            OsStr::new("--quiet"),
            OsStr::new("--"),
            path.as_os_str(),
        ],
    )
    .unwrap();

    let repo = TestRepo::new(path);
    let head = format!("refs/heads/{initial_head}");
    repo.run(["symbolic-ref", "HEAD", head.as_str()]).unwrap();
    repo
}

pub fn clone_repo(source: &Path, destination: &Path) -> TestRepo {
    checked_git(
        None,
        [
            OsStr::new("clone"),
            OsStr::new("--quiet"),
            OsStr::new("--"),
            source.as_os_str(),
            destination.as_os_str(),
        ],
    )
    .unwrap();
    TestRepo::new(destination)
}

pub fn clone_bare_repo(source: &Path, destination: &Path) -> TestRepo {
    checked_git(
        None,
        [
            OsStr::new("clone"),
            OsStr::new("--quiet"),
            OsStr::new("--bare"),
            OsStr::new("--"),
            source.as_os_str(),
            destination.as_os_str(),
        ],
    )
    .unwrap();
    TestRepo::new(destination)
}

pub fn configure_user(repo: &TestRepo) {
    repo.run(["config", "user.name", "Gitrex Test"]).unwrap();
    repo.run(["config", "user.email", "gitrex@example.com"])
        .unwrap();
    repo.run(["config", "commit.gpgSign", "false"]).unwrap();
}

pub fn commit_all(repo: &TestRepo, message: &str) -> String {
    repo.run(["add", "-A"]).unwrap();
    repo.run(["commit", "--quiet", "-m", message]).unwrap();
    repo.reference_oid("HEAD").unwrap()
}

pub fn checkout_branch(repo: &TestRepo, branch: &str) {
    repo.run(["switch", "--discard-changes", "--", branch])
        .unwrap();
}

pub fn create_branch(repo: &TestRepo, name: &str, target: &str) {
    repo.run(["branch", "--", name, target]).unwrap();
}

pub fn set_upstream(repo: &TestRepo, branch: &str, upstream: &str) {
    let upstream_arg = format!("--set-upstream-to={upstream}");
    repo.run(["branch", upstream_arg.as_str(), "--", branch])
        .unwrap();
}

pub fn set_remote_head(repo: &TestRepo, reference: &str) {
    repo.run(["symbolic-ref", "HEAD", reference]).unwrap();
}

pub fn push_branch(repo: &TestRepo, remote_name: &str, branch: &str) {
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    repo.run(["push", "--quiet", "--", remote_name, refspec.as_str()])
        .unwrap();
}

fn checked_git<I, S>(repository: Option<&Path>, args: I) -> Result<Output, TestGitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(repository, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(TestGitError(format!(
            "git command failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_output<I, S>(repository: Option<&Path>, args: I) -> Result<Output, TestGitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    if let Some(repository) = repository {
        command.arg("-C").arg(repository);
    } else {
        command.current_dir(env::temp_dir());
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| TestGitError(error.to_string()))
}
