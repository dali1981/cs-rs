use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn default_help_shows_only_canonical_command_surface() {
    let mut cmd = Command::cargo_bin("cs").expect("cs binary");
    cmd.args(["--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("backtest"))
        .stdout(predicate::str::contains("analyze").not())
        .stdout(predicate::str::contains("price").not())
        .stdout(predicate::str::contains("atm-iv").not())
        .stdout(predicate::str::contains("earnings-analysis").not())
        .stdout(predicate::str::contains("campaign").not());
}
