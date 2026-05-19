use crate::domain::{RepoStatus, StatusEntry};

use super::GitClient;

pub fn read_status(client: &GitClient) -> crate::domain::Result<RepoStatus> {
    let output = client.run_git(&["status".to_string(), "--porcelain=v1".to_string(), "--branch".to_string()])?;
    Ok(parse_status_output(&output))
}

pub fn parse_status_output(output: &str) -> RepoStatus {
    let mut lines = output.lines();
    let branch_line = lines.next().unwrap_or_default().trim_start_matches("## ").trim();

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

#[cfg(test)]
mod tests {
    use super::parse_status_output;

    #[test]
    fn parses_branch_and_files() {
        let sample = "## main...origin/main [ahead 1]\n M src/lib.rs\n?? new.txt\n";
        let status = parse_status_output(sample);
        assert_eq!(status.branch_name, "main");
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 1);
        assert_eq!(status.files.len(), 2);
    }
}
