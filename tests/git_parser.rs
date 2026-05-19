use gitrex::git::{parse_branch_lines, parse_log_lines, parse_status_output};

#[test]
fn parses_branch_log_and_status_shapes() {
    let branches = parse_branch_lines(
        "*\tmain\torigin/main\tabc123\tInitial commit\trefs/heads/main\n \tfeature/login\t\tdef456\tAdd login\trefs/heads/feature/login\n",
    );
    assert_eq!(branches.len(), 2);
    assert!(branches[0].current);

    let log = parse_log_lines("abc123\tMarcos\t2026-05-18\tInitial commit\n");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].author, "Marcos");

    let status = parse_status_output("## main...origin/main [ahead 1]\n M src/lib.rs\n");
    assert_eq!(status.branch_name, "main");
    assert_eq!(status.ahead, 1);
    assert_eq!(status.files.len(), 1);
}
