use git2::{Repository, Status, StatusOptions};

use crate::domain::{RepoStatus, StatusEntry};

use super::GitClient;

pub fn read_status(client: &GitClient) -> crate::domain::Result<RepoStatus> {
    let repo = client.repo()?;
    let head = repo.head().ok();
    let branch_name = head
        .as_ref()
        .and_then(|reference| reference.shorthand())
        .unwrap_or("HEAD")
        .to_string();
    let upstream = upstream_name(&repo, branch_name.as_str());
    let (ahead, behind) = ahead_behind(&repo, branch_name.as_str(), upstream.as_deref());
    let files = repo_status_entries(&repo)?;

    Ok(RepoStatus {
        branch_name,
        upstream,
        ahead,
        behind,
        files,
    })
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

fn upstream_name(repo: &Repository, branch_name: &str) -> Option<String> {
    let branch = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .ok()?;
    branch
        .upstream()
        .ok()?
        .name()
        .ok()
        .flatten()
        .map(|name| name.to_string())
}

fn ahead_behind(repo: &Repository, branch_name: &str, upstream: Option<&str>) -> (u32, u32) {
    let Some(upstream) = upstream else {
        return (0, 0);
    };
    let Some(local_commit) = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .ok()
        .and_then(|branch| branch.get().target())
    else {
        return (0, 0);
    };
    let Some(upstream_commit) = repo
        .revparse_single(upstream)
        .ok()
        .and_then(|object| object.peel_to_commit().ok())
        .map(|commit| commit.id())
    else {
        return (0, 0);
    };

    repo.graph_ahead_behind(local_commit, upstream_commit)
        .map(|(ahead, behind)| (ahead as u32, behind as u32))
        .unwrap_or((0, 0))
}

fn repo_status_entries(repo: &Repository) -> crate::domain::Result<Vec<StatusEntry>> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .include_ignored(false)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = repo.statuses(Some(&mut options)).map_err(map_error)?;
    Ok(statuses
        .iter()
        .filter_map(|entry| {
            let path = entry.path()?.to_string();
            let code = status_code(entry.status());
            Some(StatusEntry { code, path })
        })
        .collect())
}

fn status_code(status: Status) -> String {
    if status.contains(Status::WT_NEW) {
        return String::from("??");
    }

    let index = if status.contains(Status::INDEX_NEW) {
        'A'
    } else if status.contains(Status::INDEX_MODIFIED) {
        'M'
    } else if status.contains(Status::INDEX_DELETED) {
        'D'
    } else if status.contains(Status::INDEX_RENAMED) {
        'R'
    } else if status.contains(Status::INDEX_TYPECHANGE) {
        'T'
    } else {
        ' '
    };

    let worktree = if status.contains(Status::WT_MODIFIED) {
        'M'
    } else if status.contains(Status::WT_DELETED) {
        'D'
    } else if status.contains(Status::WT_RENAMED) {
        'R'
    } else if status.contains(Status::WT_TYPECHANGE) {
        'T'
    } else {
        ' '
    };

    format!("{index}{worktree}")
}

fn parse_branch_header(header: &str) -> (String, Option<String>, u32, u32) {
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
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let code = trimmed.get(0..2)?.to_string();
    let path = trimmed.get(3..)?.to_string();
    Some(StatusEntry { code, path })
}

fn map_error(error: git2::Error) -> crate::domain::GitError {
    if error.code() == git2::ErrorCode::NotFound {
        crate::domain::GitError::NotRepository
    } else {
        crate::domain::GitError::Backend(error.message().to_string())
    }
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
