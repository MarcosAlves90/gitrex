use git2::{BranchType, Repository};

use crate::domain::{BranchInfo, BranchKind};

use super::GitClient;
use super::shared::{map_error, short_oid};

pub fn list_branches(client: &GitClient) -> crate::domain::Result<Vec<BranchInfo>> {
    let repo = client.repo()?;
    let mut branches = Vec::new();

    collect_branches(&repo, BranchType::Local, &mut branches)?;
    collect_branches(&repo, BranchType::Remote, &mut branches)?;
    branches.sort_by(|left, right| {
        right
            .commit
            .cmp(&left.commit)
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(branches)
}

pub fn parse_branch_lines(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(6, '\t');
            let head = parts.next()?.trim();
            let name = parts.next()?.trim().to_string();
            let upstream_raw = parts.next()?.trim();
            let commit = parts.next()?.trim().to_string();
            let subject = parts.next()?.trim().to_string();
            let refname = parts.next()?.trim();

            let kind = if refname.starts_with("refs/remotes/") {
                BranchKind::Remote
            } else {
                BranchKind::Local
            };

            Some(BranchInfo {
                name,
                current: head == "*",
                upstream: (!upstream_raw.is_empty()).then(|| upstream_raw.to_string()),
                commit,
                subject,
                kind,
            })
        })
        .collect()
}

fn collect_branches(
    repo: &Repository,
    kind: BranchType,
    branches: &mut Vec<BranchInfo>,
) -> crate::domain::Result<()> {
    let head_name = repo
        .head()
        .ok()
        .and_then(|reference| reference.shorthand().map(|name| name.to_string()));

    let iter = repo.branches(Some(kind)).map_err(map_error)?;
    for item in iter {
        let (branch, branch_type) = item.map_err(map_error)?;
        let name = branch
            .name()
            .map_err(map_error)?
            .unwrap_or_default()
            .to_string();
        if matches!(kind, BranchType::Remote) && name == "HEAD" {
            continue;
        }
        let commit = branch.get().target().map(short_oid).unwrap_or_default();
        let subject = branch
            .get()
            .peel_to_commit()
            .ok()
            .and_then(|commit| commit.summary().map(|summary| summary.to_string()))
            .unwrap_or_default();
        let upstream = branch
            .upstream()
            .ok()
            .and_then(|upstream| upstream.name().ok().flatten().map(|name| name.to_string()));
        let current =
            head_name.as_deref() == Some(name.as_str()) && matches!(branch_type, BranchType::Local);

        branches.push(BranchInfo {
            name,
            current,
            upstream,
            commit,
            subject,
            kind: if matches!(kind, BranchType::Remote) {
                BranchKind::Remote
            } else {
                BranchKind::Local
            },
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_branch_lines;

    #[test]
    fn parses_basic_branch_list_output() {
        let sample = "*\tmain\torigin/main\tabc123\tInitial commit\trefs/heads/main\n \tfeature/login\t\tdef456\tAdd login\trefs/heads/feature/login\n";
        let branches = parse_branch_lines(sample);
        assert_eq!(branches.len(), 2);
        assert!(branches[0].current);
        assert_eq!(branches[0].name, "main");
    }
}
