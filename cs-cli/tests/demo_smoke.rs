//! Demo path smoke test.
//!
//! Verifies the zero-dependency demo backtest command runs end-to-end and
//! produces a non-empty result. This test is the safety rope for refactoring:
//! if it breaks, the public demo path is broken.
//!
//! Run with:
//!   cargo test --no-default-features --features demo -p cs-cli --test demo_smoke
//!
//! ## Single source of truth
//!
//! The demo parameters (conf, start, end) are the canonical values.
//! They must stay in sync with:
//!   - scripts/demo_command.sh  (DEMO_CONF, DEMO_START, DEMO_END variables)
//!   - README.md  (quick-start command block)
//!
//! If you change the dates, update all three locations.
//!
//! Protected invariants (do not weaken without a deliberate decision):
//!   1. Process exits successfully (exit code 0).
//!   2. Stdout contains "Results:" — the output section header is always printed.
//!   3. Stdout contains "Sessions Processed" — at least one session ran.
//!   4. Stdout contains "NVDA" — the demo symbol produced at least one trade line.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

// ── Canonical demo parameters ────────────────────────────────────────────────
// Must match scripts/demo_command.sh and README.md.
const DEMO_CONF:  &str = "configs/demo.toml";
const DEMO_START: &str = "2024-08-14";
const DEMO_END:   &str = "2024-08-28";
// ─────────────────────────────────────────────────────────────────────────────

/// Workspace root: cs-cli's manifest dir is cs-cli/, one level up is the root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cs-cli has a parent directory")
        .to_path_buf()
}

/// Verify the parameters in this file match scripts/demo_command.sh.
/// Fails fast with a clear message if they drift.
#[test]
fn demo_params_match_shell_script() {
    let root = workspace_root();
    let script = root.join("scripts/demo_command.sh");
    assert!(script.exists(), "scripts/demo_command.sh not found at {:?}", root);

    let content = std::fs::read_to_string(&script)
        .expect("could not read scripts/demo_command.sh");

    assert!(
        content.contains(&format!("DEMO_CONF=\"{}\"", DEMO_CONF)),
        "DEMO_CONF mismatch: expected '{}' in scripts/demo_command.sh", DEMO_CONF
    );
    assert!(
        content.contains(&format!("DEMO_START=\"{}\"", DEMO_START)),
        "DEMO_START mismatch: expected '{}' in scripts/demo_command.sh", DEMO_START
    );
    assert!(
        content.contains(&format!("DEMO_END=\"{}\"", DEMO_END)),
        "DEMO_END mismatch: expected '{}' in scripts/demo_command.sh", DEMO_END
    );
}

/// Canonical demo run. If this test fails, the demo path is broken — fix it.
#[test]
fn demo_backtest_exits_successfully_and_produces_results() {
    let root = workspace_root();

    // Verify the config and fixtures exist before running — gives a clear error
    // if the test environment is missing required files.
    assert!(
        root.join(DEMO_CONF).exists(),
        "{} not found at {:?}", DEMO_CONF, root
    );
    assert!(
        root.join("fixtures").exists(),
        "fixtures/ directory not found at {:?} — demo data is missing", root
    );

    let mut cmd = Command::cargo_bin("cs").unwrap();
    cmd.current_dir(&root)
        .args([
            "backtest",
            "--conf",  DEMO_CONF,
            "--start", DEMO_START,
            "--end",   DEMO_END,
        ]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Results:"))
        .stdout(predicate::str::contains("Sessions Processed"))
        .stdout(predicate::str::contains("NVDA"));
}
