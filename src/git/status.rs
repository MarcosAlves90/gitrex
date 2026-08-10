use crate::domain::{RepoStatus, StatusEntry};

use super::GitClient;

pub fn read_status(client: &GitClient) -> crate::domain::Result<RepoStatus> {
    let git = client.git();
    git.ensure_repository()?;
    let output = git.run_text(["status", "--porcelain=v1", "--branch"])?;
    Ok(parse_status_output(&output))
}

pub fn parse_status_output(output: &str) -> RepoStatus {
    let mut lines = output.lines();
    let branch_line = lines
        .next()
        .unwrap_or_default()
        .trim_start_matches("## ")
        .trim();

    let (branch_name, upstream, ahead, behind) = parse_branch_header(branch_line);
    let files = lines
        .filter_map(parse_status_entry)
        .collect::<Vec<StatusEntry>>();

    RepoStatus {
        branch_name,
        upstream,
        ahead,
        behind,
        files,
    }
}

fn parse_branch_header(header: &str) -> (String, Option<String>, u32, u32) {
    if let Some(branch) = header.strip_prefix("No commits yet on ") {
        return (branch.to_string(), None, 0, 0);
    }
    if header.starts_with("HEAD (") {
        return (String::from("HEAD"), None, 0, 0);
    }

    let mut branch_name = header.to_string();
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;

    if let Some((name, rest)) = header.split_once("...") {
        branch_name = name.to_string();
        let rest = rest.trim();
        if let Some((upstream_name, trailer)) = rest.split_once(' ') {
            upstream = Some(upstream_name.to_string());
            if let Some(bounds) = trailer.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                for part in bounds.split(',').map(str::trim) {
                    if let Some(value) = part.strip_prefix("ahead ") {
                        ahead = value.parse().unwrap_or(0);
                    } else if let Some(value) = part.strip_prefix("behind ") {
                        behind = value.parse().unwrap_or(0);
                    }
                }
            }
        } else if !rest.is_empty() {
            upstream = Some(rest.to_string());
        }
    }

    (branch_name, upstream, ahead, behind)
}

fn parse_status_entry(line: &str) -> Option<StatusEntry> {
    if line.is_empty() {
        return None;
    }
    let code = line.get(0..2)?.to_string();
    let path = line.get(3..)?.to_string();
    Some(StatusEntry { code, path })
}

#[cfg(test)]
mod tests {
    use super::read_status;
    use crate::git::GitClient;
    use crate::test_support::{
        commit_all, configure_user, current_dir_lock, init_repo, write_file, CurrentDirGuard,
    };

    #[test]
    fn parses_branch_and_files() {
        let sample = "## main...origin/main [ahead 1]\n M src/lib.rs\n?? new.txt\n";
        let status = super::parse_status_output(sample);
        assert_eq!(status.branch_name, "main");
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 1);
        assert_eq!(status.files.len(), 2);
        assert_eq!(status.files[0].code, " M");
        assert_eq!(status.files[0].path, "src/lib.rs");
    }

    #[test]
    fn parses_unborn_and_detached_branch_headers() {
        let unborn = super::parse_status_output("## No commits yet on main\n");
        assert_eq!(unborn.branch_name, "main");
        assert!(unborn.upstream.is_none());

        let detached = super::parse_status_output("## HEAD (no branch)\n");
        assert_eq!(detached.branch_name, "HEAD");
        assert!(detached.upstream.is_none());
    }

    #[test]
    fn reads_status_from_real_repo() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "base");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        let status = read_status(&client).unwrap();
        assert_eq!(status.branch_name, "main");
    }
}
