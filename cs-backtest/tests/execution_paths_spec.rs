use std::path::Path;

#[test]
fn execution_paths_doc_exists_and_has_required_sections() {
    let docs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
    let execution_paths = docs_dir.join("execution_paths.md");

    assert!(
        execution_paths.exists(),
        "missing {}",
        execution_paths.display()
    );

    let text = std::fs::read_to_string(&execution_paths).expect("read execution_paths.md");
    let lower = text.to_lowercase();

    assert!(
        text.contains("cargo run -p cs-cli --bin cs -- backtest"),
        "expected canonical `cs backtest` command in execution_paths.md"
    );
    assert!(
        text.contains("CLI -> config -> use_case -> services -> domain -> adapters"),
        "expected canonical production path in execution_paths.md"
    );
    assert!(
        lower.contains("production"),
        "expected `production` classification"
    );
    assert!(
        lower.contains("test-support"),
        "expected `test-support` classification"
    );
    assert!(
        lower.contains("benchmark"),
        "expected `benchmark` classification"
    );
    assert!(
        lower.contains("experimental"),
        "expected `experimental` classification"
    );
    assert!(lower.contains("dead"), "expected `dead` classification");
    assert!(
        lower.contains("remove") || lower.contains("quarantine"),
        "expected dead/misleading proposed action (`remove` or `quarantine`)"
    );
}
