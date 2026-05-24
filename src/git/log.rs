use crate::domain::CommitSummary;

use super::GitClient;

pub fn read_log(client: &GitClient, limit: usize) -> crate::domain::Result<Vec<CommitSummary>> {
    read_log_with_limit(client, Some(limit))
}

pub fn read_log_all(client: &GitClient) -> crate::domain::Result<Vec<CommitSummary>> {
    read_log_with_limit(client, None)
}

fn read_log_with_limit(
    client: &GitClient,
    limit: Option<usize>,
) -> crate::domain::Result<Vec<CommitSummary>> {
    let output = match limit {
        Some(limit) => client.run_git(&[
            "log".to_string(),
            format!("-n{limit}"),
            "--date=short".to_string(),
            "--pretty=format:%H%x09%an%x09%ad%x09%s".to_string(),
        ])?,
        None => client.run_git(&[
            "log".to_string(),
            "--date=short".to_string(),
            "--pretty=format:%H%x09%an%x09%ad%x09%s".to_string(),
        ])?,
    };
    Ok(parse_log_lines(&output))
}

pub fn parse_log_lines(output: &str) -> Vec<CommitSummary> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            Some(CommitSummary {
                hash: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                subject: parts.next()?.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_log_lines;

    #[test]
    fn parses_log_rows() {
        let sample = "abc123\tMarcos\t2026-05-18\tInitial commit\n";
        let entries = parse_log_lines(sample);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].subject, "Initial commit");
    }
}
