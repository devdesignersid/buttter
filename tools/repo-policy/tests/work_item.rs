#![cfg(unix)]

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const VALID_PR_BODY: &str = r#"## Related Issue

- Closes #9

## Acceptance Criteria

- [x] Linked work items are checked before implementation.

## Scope

### Included

- Work-item readiness validation.

### Excluded

- None: application development is excluded.

## Pre-Completion Review

- [x] Requirement fit: Every acceptance criterion is implemented and verified.
- [x] Boundaries and regressions: Validation remains limited to repository policy.
- [x] Design: Existing policy parsing remains responsible for validation.
- [x] Tests: Valid and malformed records are covered.
- [x] Correctness: Parsed records were inspected.
- [x] Standards: Repository quality gates pass.
- [x] Scope: Only approved files changed.
- [x] Maintainability: The record format is documented.
- [x] Side effects: Invalid bodies fail the existing check.
- [x] Documentation: Policy documentation is current.
- [x] Evidence: Evidence records follow below.
- [x] Unverified items: No items remain unverified.

## Evidence

### Repository Evidence

- File: `tools/repo-policy/tests/work item.rs:L1-L10`

### External Evidence

- Not applicable: API behavior is represented by test fixtures.

### Performance Evidence

- Not applicable: this is a CI policy.

## Executed Quality Gates

| Gate or command | Result |
| --- | --- |
| `cargo test` | PASS |

## Human Line Review

- [x] The user has read every line of repository-authored changes.
- Generated files excluded from line review: None.
- Third-party source excluded from line review: None.
- Concerns requiring additional review: None.

## Unverified Items

- None
"#;

const VALID_ISSUE_BODY: &str = r#"### Problem

Incomplete work items can be implemented.

### Desired observable outcome

CI accepts only ready work items.

### Acceptance criteria

- [ ] Reject an issue with a missing required section.

### Non-goals

- Judging requirement quality.

### Decisions needed

None

### Evidence plan

Use API fixtures for accepted and rejected timelines.

### Applicable quality gates

- Formatting, Clippy, tests, and 100% line coverage.

### Review scope

- Policy CLI, workflow, tests, and documentation.
"#;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-work-item-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_repo-policy"))
}

fn fake_gh() -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new();
    let path = directory.path().join("gh");
    fs::write(
        &path,
        r###"#!/bin/sh
endpoint=$2
if [ "${FAIL_ENDPOINT-}" = "$endpoint" ] || [ "${FAIL_QUERY-}" = "$4" ]; then
  printf 'simulated API failure for %s\n' "$endpoint" >&2
  exit 1
fi
case "$endpoint" in
  repos/devdesignersid/buttter/issues/9)
    case "$4" in
      *pull_request*) printf '%s\n' "${ISSUE_STATE-open}" ;;
      .body) printf '%s\n' "$ISSUE_BODY" ;;
      *implementation-approved*) printf '%s\n' "${CURRENT_LABEL-true}" ;;
      *) printf 'unexpected issue query: %s\n' "$4" >&2; exit 92 ;;
    esac
    ;;
  repos/devdesignersid/buttter/issues/9/timeline)
    found_paginate=false
    for argument in "$@"; do
      [ "$argument" = --paginate ] && found_paginate=true
    done
    [ "$found_paginate" = true ] || { printf 'timeline was not paginated\n' >&2; exit 93; }
    printf '%b' "${TIMELINE-labeled\t2026-09-22T06:53:07Z\tdevdesignersid\n}"
    ;;
  repos/devdesignersid/buttter/pulls/10)
    case "$4" in
      .created_at) printf '%s\n' "${PR_CREATED_AT-2026-09-22T06:53:08Z}" ;;
      .head.sha)
        [ "${REMOVE_GH-}" = true ] && /bin/rm "$0"
        printf '%s\n' "${HEAD_SHA-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}"
        ;;
      *) printf 'unexpected pull query: %s\n' "$4" >&2; exit 94 ;;
    esac
    ;;
  repos/devdesignersid/buttter/contents/tools/repo-policy/tests/work%20item.rs\?ref=*)
    printf '%b' "${FILE_CONTENT-1\\n2\\n3\\n4\\n5\\n6\\n7\\n8\\n9\\n10\\n}"
    ;;
  *) printf 'unexpected endpoint: %s\n' "$endpoint" >&2; exit 91 ;;
esac
"###,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
    directory
}

fn run_with_api(environment: &[(&str, &str)]) -> Output {
    let directory = fake_gh();
    let mut child = command()
        .args([
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("ISSUE_BODY", VALID_ISSUE_BODY)
        .envs(environment.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(VALID_PR_BODY.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn assert_rejected(environment: &[(&str, &str)], diagnostic: &str) {
    let output = run_with_api(environment);
    assert!(!output.status.success(), "policy unexpectedly passed");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
        "missing `{diagnostic}` in {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn replace_once(source: &str, from: &str, to: &str) -> String {
    assert_eq!(source.matches(from).count(), 1);
    source.replacen(from, to, 1)
}

#[test]
fn accepts_a_complete_work_item_approved_before_pull_request_creation() {
    let output = run_with_api(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Pull request body is valid.\n");
}

#[test]
fn validates_repository_evidence_against_the_pull_request_head() {
    assert_rejected(&[("HEAD_SHA", "invalid")], "head revision");
    assert_rejected(&[("FILE_CONTENT", "one\\ntwo\\n")], "line range");
    assert_rejected(&[("FILE_CONTENT", "\\377")], "UTF-8");
    assert_rejected(&[("REMOVE_GH", "true")], "could not start GitHub CLI");
    assert_rejected(
        &[(
            "FAIL_ENDPOINT",
            "repos/devdesignersid/buttter/contents/tools/repo-policy/tests/work%20item.rs?ref=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
        "simulated API failure",
    );
}

#[test]
fn rejects_missing_duplicate_reordered_and_additional_work_item_headings() {
    let bodies = [
        VALID_ISSUE_BODY.replace("### Review scope\n", ""),
        VALID_ISSUE_BODY.replace("### Review scope\n", "### Problem\n\n### Review scope\n"),
        VALID_ISSUE_BODY
            .replace("### Problem", "### Outcome TEMP")
            .replace("### Desired observable outcome", "### Problem")
            .replace("### Outcome TEMP", "### Desired observable outcome"),
        VALID_ISSUE_BODY.replace(
            "### Evidence plan\n",
            "### Unexpected\n\nText.\n\n### Evidence plan\n",
        ),
    ];

    for body in &bodies {
        assert_rejected(&[("ISSUE_BODY", body)], "work-item headings");
    }
}

#[test]
fn rejects_empty_placeholder_or_malformed_work_item_content() {
    let cases = [
        (
            "Incomplete work items can be implemented.",
            "<!-- prompt -->",
            "Problem",
        ),
        (
            "CI accepts only ready work items.",
            "-",
            "Desired observable outcome",
        ),
        (
            "- [ ] Reject an issue with a missing required section.",
            "- [ ]",
            "Acceptance criteria",
        ),
        (
            "- [ ] Reject an issue with a missing required section.",
            "Reject missing sections.",
            "Acceptance criteria",
        ),
        ("- Judging requirement quality.", "-", "Non-goals"),
        ("None", "Choose an approver.", "Decisions needed"),
        (
            "Use API fixtures for accepted and rejected timelines.",
            "<!-- TODO -->",
            "Evidence plan",
        ),
        (
            "- Formatting, Clippy, tests, and 100% line coverage.",
            "- [ ]",
            "Applicable quality gates",
        ),
        (
            "- Policy CLI, workflow, tests, and documentation.",
            "-",
            "Review scope",
        ),
    ];

    for (from, to, diagnostic) in cases {
        let body = replace_once(VALID_ISSUE_BODY, from, to);
        assert_rejected(&[("ISSUE_BODY", &body)], diagnostic);
    }
}

#[test]
fn accepts_explicit_none_for_non_goals_and_checked_or_unchecked_criteria() {
    let body = replace_once(VALID_ISSUE_BODY, "- Judging requirement quality.", "None");
    let body = replace_once(&body, "- [ ] Reject", "- [x] Reject");
    let output = run_with_api(&[("ISSUE_BODY", &body)]);
    assert!(output.status.success());
}

#[test]
fn rejects_non_open_issues_and_missing_current_approval_label() {
    assert_rejected(&[("ISSUE_STATE", "closed")], "not open");
    assert_rejected(&[("ISSUE_STATE", "pull_request")], "pull request");
    assert_rejected(&[("ISSUE_STATE", "unknown")], "unexpected response");
    assert_rejected(&[("CURRENT_LABEL", "false")], "does not currently have");
    assert_rejected(&[("CURRENT_LABEL", "unknown")], "unexpected response");
}

#[test]
fn rejects_missing_removed_and_unauthorized_approval_records() {
    assert_rejected(&[("TIMELINE", "")], "approval timeline");
    assert_rejected(
        &[(
            "TIMELINE",
            "labeled\\t2026-09-22T06:53:05Z\\tdevdesignersid\\nunlabeled\\t2026-09-22T06:53:06Z\\tdevdesignersid\\n",
        )],
        "latest approval-label event is not an application",
    );
    assert_rejected(
        &[(
            "TIMELINE",
            "labeled\\t2026-09-22T06:53:07Z\\tunauthorized-user\\n",
        )],
        "not authorized",
    );
}

#[test]
fn uses_the_latest_timeline_event_regardless_of_api_order() {
    let output = run_with_api(&[(
        "TIMELINE",
        "labeled\\t2026-09-22T06:53:07Z\\tdevdesignersid\\nunlabeled\\t2026-09-22T06:53:06Z\\tdevdesignersid\\n",
    )]);
    assert!(output.status.success());
}

#[test]
fn requires_approval_to_strictly_predate_pull_request_creation() {
    assert_rejected(
        &[(
            "TIMELINE",
            "labeled\\t2026-09-22T06:53:08Z\\tdevdesignersid\\n",
        )],
        "must predate",
    );
    assert_rejected(
        &[(
            "TIMELINE",
            "labeled\\t2026-09-22T06:53:09Z\\tdevdesignersid\\n",
        )],
        "must predate",
    );
}

#[test]
fn rejects_malformed_timeline_rows_and_timestamps() {
    for timeline in [
        "labeled\\tbad-time\\tdevdesignersid\\n",
        "labeled\\t2026-02-30T06:53:07Z\\tdevdesignersid\\n",
        "labeled\\t2026-09-22T06:53:07Z\\n",
        "labeled\\t2026-09-22T06:53:07Z\\t\\n",
        "labeled\\t2026-13-22T06:53:07Z\\tdevdesignersid\\n",
        "unexpected\\t2026-09-22T06:53:07Z\\tdevdesignersid\\n",
    ] {
        assert_rejected(&[("TIMELINE", timeline)], "malformed approval timeline");
    }
    assert_rejected(&[("PR_CREATED_AT", "not-a-time")], "pull request timestamp");
}

#[test]
fn accepts_valid_timestamps_across_month_and_leap_year_boundaries() {
    for (timeline, pull_created_at) in [
        (
            "labeled\\t2026-01-31T23:59:59Z\\tdevdesignersid\\n",
            "2026-02-01T00:00:00Z",
        ),
        (
            "labeled\\t2026-04-30T23:59:59Z\\tdevdesignersid\\n",
            "2026-05-01T00:00:00Z",
        ),
        (
            "labeled\\t2000-02-29T23:59:59Z\\tdevdesignersid\\n",
            "2000-03-01T00:00:00Z",
        ),
    ] {
        let output = run_with_api(&[("TIMELINE", timeline), ("PR_CREATED_AT", pull_created_at)]);
        assert!(output.status.success());
    }
}

#[test]
fn fails_closed_for_every_github_api_request() {
    for endpoint in [
        "repos/devdesignersid/buttter/issues/9",
        "repos/devdesignersid/buttter/issues/9/timeline",
        "repos/devdesignersid/buttter/pulls/10",
    ] {
        assert_rejected(&[("FAIL_ENDPOINT", endpoint)], "simulated API failure");
    }
    for query in [
        ".body",
        "[.labels[].name] | index(\"implementation-approved\") != null",
    ] {
        assert_rejected(&[("FAIL_QUERY", query)], "simulated API failure");
    }
}

#[test]
fn requires_valid_and_complete_online_arguments() {
    for args in [
        vec!["validate-pr-body", "--pull-request-number"],
        vec!["validate-pr-body", "--pull-request-number", "10"],
        vec![
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "0",
        ],
        vec![
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "invalid",
        ],
        vec![
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
            "--pull-request-number",
            "11",
        ],
    ] {
        let mut child = command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child
            .stdin
            .take()
            .unwrap()
            .write_all(VALID_PR_BODY.as_bytes())
        {
            assert_eq!(
                error.kind(),
                ErrorKind::BrokenPipe,
                "write body to child stdin: {error}"
            );
        }
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}
