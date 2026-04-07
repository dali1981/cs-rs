//! Demo path smoke test.
//!
//! Verifies the zero-dependency demo backtest command runs end-to-end and
//! produces a non-empty result. This test is the safety rope for refactoring:
//! if it breaks, the public demo path is broken.
//!
//! Run with:
//!   cargo test --no-default-features --features demo -p cs-cli --test demo_smoke
//!
//! Protected invariants (do not weaken without a deliberate decision):
//!   1. Process exits successfully (exit code 0).
//!   2. Stdout contains "Results:" — the output section header is always printed.
//!   3. Stdout contains "Sessions Processed" — at least one session ran.
//!   4. Stdout contains "NVDA" — the demo symbol produced at least one trade line.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

/// Workspace root: cs-cli's manifest dir is cs-cli/, one level up is the root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cs-cli has a parent directory")
        .to_path_buf()
}

/// Canonical demo command verified against fixtures/nvda_options.parquet.
///
/// Dates: NVDA Aug-2024 earnings (2024-08-28 AMC).
/// Entry: 6 trading days before = ~2024-08-20.
/// Config: configs/demo.toml (strategy, timing, selection parameters).
///
/// If this test fails, the demo path is broken. Do not skip it — fix the demo.
#[test]
fn demo_backtest_exits_successfully_and_produces_results() {
    let root = workspace_root();

    // Verify the config and fixtures exist before running — gives a clear error
    // if the test environment is missing required files.
    assert!(
        root.join("configs/demo.toml").exists(),
        "configs/demo.toml not found at {:?}",
        root
    );
    assert!(
        root.join("fixtures").exists(),
        "fixtures/ directory not found at {:?} — demo data is missing",
        root
    );

    let mut cmd = Command::cargo_bin("cs").unwrap();
    cmd.current_dir(&root)
        .args([
            "backtest",
            "--conf", "configs/demo.toml",
            "--start", "2024-08-14",
            "--end",   "2024-08-28",
        ]);

    cmd.assert()
        .success()
        // Output section header — always printed when backtest completes
        .stdout(predicate::str::contains("Results:"))
        // Results table — present when at least one session was processed
        .stdout(predicate::str::contains("Sessions Processed"))
        // Trade line — present when at least one trade was executed
        .stdout(predicate::str::contains("NVDA"));
}
