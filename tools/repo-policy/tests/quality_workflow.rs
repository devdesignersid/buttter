use std::fs;
use std::path::Path;
use std::process::Command;

fn workflow() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join("../../.github/workflows/repo-policy-quality.yml")).unwrap()
}

#[test]
fn repository_manifest_is_valid() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_repo-policy"))
        .args(["validate-quality-manifest", ".github/quality-targets.toml"])
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runs_for_every_pull_request_and_main_push_without_path_filters() {
    let workflow = workflow();

    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("push:\n    branches: [main]"));
    assert!(!workflow.contains("    paths:"));
    assert!(workflow.contains("permissions:\n  contents: read"));
    assert!(workflow.contains("fetch-depth: 0"));
}

#[test]
fn plans_and_runs_every_registered_platform_target() {
    let workflow = workflow();

    assert!(workflow.contains("validate-quality-manifest .github/quality-targets.toml"));
    assert!(workflow.contains("quality-matrix .github/quality-targets.toml"));
    assert!(workflow.contains("fromJSON(needs.plan.outputs.matrix)"));
    assert!(workflow.contains("runs-on: ${{ matrix.runner }}"));
    assert!(
        workflow.contains("run-quality-target .github/quality-targets.toml ${{ matrix.name }}")
    );
}

#[test]
fn pins_tools_and_publishes_one_stable_aggregate_job() {
    let workflow = workflow();

    assert!(workflow.contains("rust@1.98.1 + rustfmt + clippy"));
    assert!(workflow.contains("cargo-llvm-cov@0.9.1"));
    assert!(!workflow.contains("cargo-mutants"));
    assert!(workflow.contains("name: Repository quality"));
    assert!(workflow.contains("needs: [plan, quality]"));
    assert!(workflow.contains("if: always()"));
    assert!(workflow.contains("PLAN_RESULT: ${{ needs.plan.result }}"));
    assert!(workflow.contains("QUALITY_RESULT: ${{ needs.quality.result }}"));
    assert!(workflow.contains("test \"$PLAN_RESULT\" = success"));
    assert!(workflow.contains("test \"$QUALITY_RESULT\" = success"));
}
