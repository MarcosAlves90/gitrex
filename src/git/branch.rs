use crate::domain::{BranchInfo, BranchKind};

use super::GitClient;

pub fn list_branches(client: &GitClient) -> crate::domain::Result<Vec<BranchInfo>> {
    let output = client.run_git(&[
        "for-each-ref".to_string(),
        "--sort=-committerdate".to_string(),
        "--format=%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(objectname:short)\t%(subject)\t%(refname)".to_string(),
        "refs/heads".to_string(),
        "refs/remotes".to_string(),
    ])?;
    Ok(parse_branch_lines(&output))
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
