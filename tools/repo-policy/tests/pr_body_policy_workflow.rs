use std::fs;
use std::path::Path;

fn workflow() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join("../../.github/workflows/pr-body-policy.yml")).unwrap()
}

#[test]
fn validates_pr_head_evidence_with_trusted_policy_code() {
    let workflow = workflow();

    assert!(workflow.contains(
        "pull_request_target:\n    types: [opened, edited, reopened, synchronize, ready_for_review]"
    ));
    assert!(
        workflow.contains("permissions:\n  contents: read\n  issues: read\n  pull-requests: read")
    );
    assert!(workflow.contains("ref: ${{ github.event.pull_request.base.sha }}"));
    assert!(workflow.contains("validate-pr-body"));
    assert!(workflow.contains("--github-repository \"$GITHUB_REPOSITORY\""));
    assert!(workflow.contains("--pull-request-number \"${{ github.event.pull_request.number }}\""));
    assert!(!workflow.contains("github.event.pull_request.head"));
}
