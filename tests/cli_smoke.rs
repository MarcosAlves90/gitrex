use predicates::str::contains;

#[test]
fn cli_help_mentions_tui_default() {
    let mut cmd = assert_cmd::Command::cargo_bin("gitrex").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(contains("Terminal-first git manager"));
}
