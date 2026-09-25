#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const DIGEST: &str = "aa3ac94e5ccd69c751bfc893aca43f0c2ca36ef15401e5ea3176f5b2d017a97a";

const VALID_PR_BODY: &str = r#"## Related Issue

- Closes #9

## Acceptance Criteria

- [x] File scope is enforced.

## Scope

### Included

- File-scope policy.

### Excluded

- None: all exclusions are listed in the work item.

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

- File: `tools/repo-policy/tests/file_scope.rs:L1-L10`

### External Evidence

- Not applicable: API behavior is represented by test fixtures.

### Performance Evidence

- Not applicable: policy validation is bounded.

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

Files can exceed approved scope.

### Desired observable outcome

Only exact approved paths change.

### Acceptance criteria

- [ ] Reject unapproved paths.

### Non-goals

- Semantic review.

### Decisions needed

None

### Evidence plan

Use API fixtures.

### Applicable quality gates

- Tests and coverage.

### Review scope

Review every listed file.

<!-- approved-paths:start -->
```json
[
  ".config",
  "generated.lock",
  "line\nbreak.txt",
  "src/lib.rs",
  "unicode/é.txt",
  "white space.txt"
]
```
<!-- approved-paths:end -->
"#;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-file-scope-test-{}-{id}",
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

fn percent_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(*byte).to_string()
            }
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

fn file_record(status: &str, path: &str, previous: &str) -> String {
    format!(
        "{}\t{}\t{}\n",
        percent_encode(status),
        percent_encode(path),
        percent_encode(previous)
    )
}

fn approval_record(id: u64, actor: &str, timestamp: &str, body: &str) -> String {
    approval_record_updated(id, actor, timestamp, timestamp, body)
}

fn approval_record_updated(
    id: u64,
    actor: &str,
    created_at: &str,
    updated_at: &str,
    body: &str,
) -> String {
    format!(
        "{id}\t{}\t{}\t{}\t{}\n",
        percent_encode(actor),
        percent_encode(created_at),
        percent_encode(updated_at),
        percent_encode(body)
    )
}

fn fake_gh() -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new();
    let path = directory.path().join("gh");
    fs::write(
        &path,
        r###"#!/bin/sh
endpoint=$2
query=$4
if [ "${FAIL_ENDPOINT-}" = "$endpoint" ] || { [ "${FAIL_QUERY-}" = "$query" ] && { [ -z "${FAIL_QUERY_ENDPOINT-}" ] || [ "${FAIL_QUERY_ENDPOINT-}" = "$endpoint" ]; }; }; then
  printf 'simulated API failure for %s\n' "$endpoint" >&2
  exit 1
fi
case "$endpoint" in
  repos/devdesignersid/buttter/pulls/10)
    case "$query" in
      .body) printf '%s\n' "$PR_BODY" ;;
      *changed_files*) printf '%s\n' "${CHANGED_FILES-1}" ;;
      .created_at) printf '%s\n' "${PR_CREATED_AT-2026-09-22T06:53:08Z}" ;;
      *) printf 'unexpected pull query: %s\n' "$query" >&2; exit 92 ;;
    esac
    ;;
  repos/devdesignersid/buttter/issues/9)
    case "$query" in
      *pull_request*) printf '%s\n' "${ISSUE_STATE-open}" ;;
      .body) printf '%s\n' "$ISSUE_BODY" ;;
      *) printf 'unexpected issue query: %s\n' "$query" >&2; exit 93 ;;
    esac
    ;;
  repos/devdesignersid/buttter/issues/9/comments)
    found_paginate=false
    for argument in "$@"; do [ "$argument" = --paginate ] && found_paginate=true; done
    [ "$found_paginate" = true ] || { printf 'comments were not paginated\n' >&2; exit 94; }
    printf '%b' "$APPROVAL_RECORDS"
    ;;
  repos/devdesignersid/buttter/pulls/10/files)
    found_paginate=false
    for argument in "$@"; do [ "$argument" = --paginate ] && found_paginate=true; done
    [ "$found_paginate" = true ] || { printf 'files were not paginated\n' >&2; exit 95; }
    printf '%b' "${FILE_RECORDS-modified\tsrc/lib.rs\t\n}"
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
    let approval = approval_record(
        100,
        "devdesignersid",
        "2026-09-22T06:53:07Z",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    command()
        .args([
            "validate-pr-file-scope",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("PR_BODY", VALID_PR_BODY)
        .env("ISSUE_BODY", VALID_ISSUE_BODY)
        .env("APPROVAL_RECORDS", approval)
        .envs(environment.iter().copied())
        .output()
        .unwrap()
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

fn issue_with_json(json: &str) -> String {
    let start = VALID_ISSUE_BODY.find("[\n").unwrap();
    let end = VALID_ISSUE_BODY[start..].find("\n]\n").unwrap() + start + 2;
    format!(
        "{}{}{}",
        &VALID_ISSUE_BODY[..start],
        json,
        &VALID_ISSUE_BODY[end..]
    )
}

#[test]
fn accepts_every_supported_status_and_unusual_exact_paths() {
    for (status, path, previous) in [
        ("added", ".config", ""),
        ("modified", "generated.lock", ""),
        ("removed", "line\nbreak.txt", ""),
        ("changed", "unicode/é.txt", ""),
        ("unchanged", "white space.txt", ""),
        ("renamed", "src/lib.rs", ".config"),
        ("copied", "src/lib.rs", "generated.lock"),
    ] {
        let records = file_record(status, path, previous);
        let output = run_with_api(&[("FILE_RECORDS", &records)]);
        assert!(
            output.status.success(),
            "{status}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"Pull request file scope is valid.\n");
    }
}

#[test]
fn accepts_an_empty_diff_deterministically() {
    let output = run_with_api(&[("CHANGED_FILES", "0"), ("FILE_RECORDS", "")]);
    assert!(output.status.success());
}

#[test]
fn rejects_paths_outside_scope_and_requires_both_rename_or_copy_paths() {
    assert_rejected(
        &[("FILE_RECORDS", "modified\tdocs/outside.md\t\n")],
        "outside approved scope",
    );
    for status in ["renamed", "copied"] {
        let records = file_record(status, "src/lib.rs", "docs/outside.md");
        assert_rejected(&[("FILE_RECORDS", &records)], "outside approved scope");
        let records = file_record(status, "src/lib.rs", "");
        assert_rejected(&[("FILE_RECORDS", &records)], "previous path");
    }
}

#[test]
fn rejects_missing_malformed_duplicate_unsorted_or_broad_declarations() {
    let cases = [
        (
            VALID_ISSUE_BODY.replace("<!-- approved-paths:start -->", ""),
            "markers",
        ),
        (VALID_ISSUE_BODY.replace("```json", "```yaml"), "JSON fence"),
        (
            VALID_ISSUE_BODY
                .replace(
                    "<!-- approved-paths:start -->",
                    "<!-- approved-paths:end TEMP -->",
                )
                .replace(
                    "<!-- approved-paths:end -->",
                    "<!-- approved-paths:start -->",
                )
                .replace(
                    "<!-- approved-paths:end TEMP -->",
                    "<!-- approved-paths:end -->",
                ),
            "start-to-end order",
        ),
        (issue_with_json("{}"), "JSON array"),
        (issue_with_json("[\"src/lib.rs\",]"), "JSON array"),
        (
            issue_with_json("[\"src/lib.rs\", \"src/lib.rs\"]"),
            "duplicate",
        ),
        (issue_with_json("[\"src/lib.rs\", \".config\"]"), "sorted"),
        (issue_with_json("[]"), "one or more"),
        (issue_with_json("[\"src/**\"]"), "glob"),
        (issue_with_json("[\"/src/lib.rs\"]"), "repository-relative"),
        (
            issue_with_json("[\"src/../lib.rs\"]"),
            "repository-relative",
        ),
        (issue_with_json("[\"src/\"]"), "file path"),
        (issue_with_json("[\"src//lib.rs\"]"), "repository-relative"),
        (issue_with_json("[\"bad\\u0000path\"]"), "NUL"),
        (issue_with_json("[\"bad\\xescape\"]"), "JSON array"),
        (issue_with_json("[\"unterminated]"), "JSON array"),
        (issue_with_json("[\"incomplete\\"), "JSON array"),
        (issue_with_json("[\"bad\\u12\"]"), "JSON array"),
        (issue_with_json("[\"bad\\u12"), "JSON array"),
        (issue_with_json("[\"ok\"] trailing"), "JSON array"),
        (issue_with_json("[\"\"]"), "repository-relative"),
    ];
    for (body, diagnostic) in &cases {
        assert_rejected(&[("ISSUE_BODY", body)], diagnostic);
    }
}

#[test]
fn parses_json_escapes_without_weakening_exact_matching() {
    let body = issue_with_json(
        "[\"backspace\\bpath\", \"formfeed\\fpath\", \"line\\npath\", \"quote\\\"path\", \"return\\rpath\", \"slash\\/path\", \"slash\\\\path\", \"tab\\tpath\", \"unicode\\u002Fpath\", \"𐀀\", \"\\uD801\\uDC00\"]",
    );
    assert_rejected(&[("ISSUE_BODY", &body)], "scope approval digest");

    for malformed in [
        "[\"bad\\uD800\"]",
        "[\"bad\\uDC00\"]",
        "[\"bad\\uD800x\"]",
        "[\"bad\npath\"]",
    ] {
        let body = issue_with_json(malformed);
        assert_rejected(&[("ISSUE_BODY", &body)], "JSON array");
    }
    let invalid_pair = issue_with_json("[\"bad\\uD800\\u0041\"]");
    assert_rejected(
        &[("ISSUE_BODY", &invalid_pair)],
        "surrogate pair contains an invalid low surrogate",
    );
}

#[test]
fn rejects_missing_malformed_unauthorized_stale_or_late_scope_approval() {
    assert_rejected(&[("APPROVAL_RECORDS", "")], "scope approval");
    assert_rejected(&[("APPROVAL_RECORDS", "malformed\n")], "malformed record");
    assert_rejected(
        &[(
            "APPROVAL_RECORDS",
            "invalid\tactor\tcreated\tupdated\tbody\n",
        )],
        "comment ID",
    );
    let duplicate = format!(
        "{}{}",
        approval_record(100, "devdesignersid", "2026-09-22T06:53:06Z", "discussion"),
        approval_record(
            100,
            "devdesignersid",
            "2026-09-22T06:53:07Z",
            &format!("scope-approved sha256:{DIGEST}")
        ),
    );
    assert_rejected(&[("APPROVAL_RECORDS", &duplicate)], "duplicate comment ID");
    let unrelated = approval_record(99, "contributor", "2026-09-22T06:53:06Z", "discussion");
    assert_rejected(&[("APPROVAL_RECORDS", &unrelated)], "no scope approval");
    for body in [
        "scope-approved",
        "scope-approved sha256:ABC",
        "scope-approved sha256:aa3ac94e5ccd69c751bfc893aca43f0c2ca36ef15401e5ea3176f5b2d017a97ax",
    ] {
        let record = approval_record(100, "devdesignersid", "2026-09-22T06:53:07Z", body);
        assert_rejected(&[("APPROVAL_RECORDS", &record)], "malformed scope approval");
    }

    let empty_actor = approval_record(
        100,
        "",
        "2026-09-22T06:53:07Z",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    assert_rejected(&[("APPROVAL_RECORDS", &empty_actor)], "malformed actor");

    let unauthorized = approval_record(
        100,
        "someone-else",
        "2026-09-22T06:53:07Z",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    assert_rejected(&[("APPROVAL_RECORDS", &unauthorized)], "not authorized");

    let stale = approval_record(
        100,
        "devdesignersid",
        "2026-09-22T06:53:07Z",
        "scope-approved sha256:0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_rejected(&[("APPROVAL_RECORDS", &stale)], "does not match");

    for timestamp in ["2026-09-22T06:53:08Z", "2026-09-22T06:53:09Z", "invalid"] {
        let record = approval_record(
            100,
            "devdesignersid",
            timestamp,
            &format!("scope-approved sha256:{DIGEST}"),
        );
        assert_rejected(&[("APPROVAL_RECORDS", &record)], "scope approval timestamp");
    }
    let invalid_update = approval_record_updated(
        100,
        "devdesignersid",
        "2026-09-22T06:53:06Z",
        "invalid",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    assert_rejected(
        &[("APPROVAL_RECORDS", &invalid_update)],
        "update timestamp is malformed",
    );
    let edited_after_creation = approval_record_updated(
        100,
        "devdesignersid",
        "2026-09-22T06:53:06Z",
        "2026-09-22T06:53:09Z",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    assert_rejected(
        &[("APPROVAL_RECORDS", &edited_after_creation)],
        "scope approval update timestamp",
    );
}

#[test]
fn the_latest_surviving_scope_approval_controls() {
    let valid = approval_record(
        100,
        "devdesignersid",
        "2026-09-22T06:53:06Z",
        &format!("scope-approved sha256:{DIGEST}"),
    );
    let stale = approval_record(
        101,
        "devdesignersid",
        "2026-09-22T06:53:07Z",
        "scope-approved sha256:0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_rejected(
        &[("APPROVAL_RECORDS", &format!("{stale}{valid}"))],
        "does not match",
    );
}

#[test]
fn rejects_incomplete_or_malformed_file_api_data() {
    assert_rejected(&[("CHANGED_FILES", "invalid")], "changed-file count");
    assert_rejected(&[("CHANGED_FILES", "3001")], "3,000-file");
    assert_rejected(&[("CHANGED_FILES", "2")], "pagination mismatch");
    assert_rejected(
        &[
            (
                "FILE_RECORDS",
                "modified\tsrc/lib.rs\t\nmodified\tsrc/lib.rs\t\n",
            ),
            ("CHANGED_FILES", "2"),
        ],
        "duplicate",
    );
    for records in [
        "malformed\n",
        "modified\t\t\n",
        "unknown\tsrc/lib.rs\t\n",
        "modified\tbad%XXpath\t\n",
        "modified\t%FF\t\n",
        "modified\tsrc/lib.rs\t.config\n",
    ] {
        assert_rejected(&[("FILE_RECORDS", records)], "file data");
    }
}

#[test]
fn fails_closed_for_issue_pull_comment_and_file_api_requests() {
    for endpoint in [
        "repos/devdesignersid/buttter/pulls/10",
        "repos/devdesignersid/buttter/issues/9",
        "repos/devdesignersid/buttter/issues/9/comments",
        "repos/devdesignersid/buttter/pulls/10/files",
    ] {
        assert_rejected(&[("FAIL_ENDPOINT", endpoint)], "simulated API failure");
    }
    assert_rejected(&[("ISSUE_STATE", "closed")], "not open");

    for (endpoint, query) in [
        ("repos/devdesignersid/buttter/issues/9", ".body"),
        ("repos/devdesignersid/buttter/pulls/10", ".created_at"),
        (
            "repos/devdesignersid/buttter/pulls/10",
            ".changed_files | if type == \"number\" then . else error(\"malformed changed-file count\") end",
        ),
    ] {
        assert_rejected(
            &[("FAIL_QUERY_ENDPOINT", endpoint), ("FAIL_QUERY", query)],
            "simulated API failure",
        );
    }
    assert_rejected(&[("PR_CREATED_AT", "invalid")], "pull request timestamp");
    let invalid_work_item = VALID_ISSUE_BODY.replace(
        "### Decisions needed\n\nNone",
        "### Decisions needed\n\nUnresolved",
    );
    assert_rejected(
        &[("ISSUE_BODY", &invalid_work_item)],
        "Linked work-item policy violations",
    );
}

#[test]
fn requires_complete_arguments_and_valid_pull_request_body_data() {
    for args in [
        vec!["validate-pr-file-scope"],
        vec![
            "validate-pr-file-scope",
            "--github-repository",
            "devdesignersid/buttter",
        ],
        vec!["validate-pr-file-scope", "--pull-request-number", "10"],
    ] {
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }

    let directory = fake_gh();
    let mut child = command()
        .args([
            "validate-pr-file-scope",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("PR_BODY", "invalid")
        .env("ISSUE_BODY", VALID_ISSUE_BODY)
        .env("APPROVAL_RECORDS", "")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"ignored").unwrap();
    assert!(!child.wait().unwrap().success());
}
