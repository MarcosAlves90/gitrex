use crate::domain::BranchHistory;

use super::GitClient;

pub fn read_branch_history(client: &GitClient, reference: &str) -> crate::domain::Result<BranchHistory> {
    let reference = if reference.starts_with("refs/") {
        reference.to_string()
    } else {
        format!("refs/heads/{reference}")
    };
    let output = client.run_git(&[
        "log".to_string(),
        reference,
        "--graph".to_string(),
        "--date=short".to_string(),
        "--pretty=format:%x09%H%x09%an%x09%ad%x09%s".to_string(),
    ])?;
    let graph = super::log::parse_graph_log_lines(&output);
    Ok(BranchHistory::from_graph(graph))
}

#[cfg(test)]
mod tests {
    use super::read_branch_history;
    use crate::git::GitClient;
    use std::{
        env,
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    #[test]
    fn reads_only_requested_branch_history() {
        let _guard = crate::test_support::current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        init_divergent_repo(temp.path());

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        let client = GitClient::new();
        let history = read_branch_history(&client, "main").unwrap();

        env::set_current_dir(original_dir).unwrap();

        assert!(history.commits.iter().any(|entry| entry.subject == "main work"));
        assert!(!history.commits.iter().any(|entry| entry.subject == "feature work"));
    }

    fn init_divergent_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        configure_repo(path);

        write_file(path, "README.md", "base\n");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "base commit"]);

        run_git(path, &["checkout", "-b", "feature/login"]);
        write_file(path, "README.md", "feature work\n");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "feature work"]);

        run_git(path, &["checkout", "main"]);
        write_file(path, "README.md", "main work\n");
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "main work"]);
    }

    fn configure_repo(path: &Path) {
        run_git(path, &["config", "user.name", "Gitrex Test"]);
        run_git(path, &["config", "user.email", "gitrex@example.com"]);
    }

    fn run_git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed in {}", args, path.display());
    }

    fn write_file(path: &Path, name: &str, contents: &str) {
        fs::write(PathBuf::from(path).join(name), contents).unwrap();
    }
}
