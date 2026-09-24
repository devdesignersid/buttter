use std::fs;
use std::path::Path;

fn workflow() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join("../../.github/workflows/mutation-testing.yml")).unwrap()
}

#[test]
fn benchmarks_issue_14_mutants_on_the_pinned_runner() {
    let workflow = workflow();

    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("permissions:\n  contents: read"));
    assert!(workflow.contains("runs-on: ubuntu-24.04"));
    assert!(workflow.contains("timeout-minutes: 5"));
    assert!(workflow.contains("rust@1.98.1"));
    assert!(workflow.contains("cargo-mutants@27.1.0"));
    assert!(workflow.contains("--in-place"));
    assert!(workflow.contains("--no-shuffle"));
    assert!(workflow.contains("--file src/main.rs"));
    assert!(workflow.contains("--re 'validate_(body|bullet_list|labeled_lines)'"));
    assert!(workflow.contains("--test pr_body"));
    assert!(workflow.contains("mutation-elapsed-seconds"));
    assert!(workflow.contains("actions/upload-artifact@"));
    assert!(workflow.contains("if: always()"));
    assert!(workflow.contains("timeout-verification:"));
    assert!(workflow.matches("timeout-minutes: 5").count() == 2);
    assert!(workflow.contains("run: sleep 360"));
    assert!(!workflow.contains("pull_request_target"));
    assert!(!workflow.contains("secrets:"));
}
