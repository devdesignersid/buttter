use std::fs;
use std::path::Path;

fn workflow() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join("../../.github/workflows/commit-message-policy.yml")).unwrap()
}

#[test]
fn reruns_for_commit_and_base_branch_changes_using_trusted_code() {
    let workflow = workflow();

    assert!(workflow.contains(
        "pull_request_target:\n    types: [opened, edited, reopened, synchronize, ready_for_review]"
    ));
    assert!(workflow.contains("ref: ${{ github.event.pull_request.base.sha }}"));
    assert!(!workflow.contains("github.event.pull_request.head"));
}
