#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const BODY: &str = r#"## Related Issue

- Closes #6

## Acceptance Criteria

- [x] Remote evidence is validated.

## Scope

### Included

- Remote evidence validation.

### Excluded

- None: all policy behavior is included.

## Pre-Completion Review

- [x] Requirement fit: The required behavior is verified.
- [x] Boundaries and regressions: Only evidence validation changes.
- [x] Design: Trusted policy code reads inert head content.
- [x] Tests: Remote success and failure paths are covered.
- [x] Correctness: Paths and ranges are checked.
- [x] Standards: Repository gates pass.
- [x] Scope: Only approved files change.
- [x] Maintainability: The syntax is documented.
- [x] Side effects: Invalid evidence fails closed.
- [x] Documentation: The contract is current.
- [x] Evidence: Evidence follows.
- [x] Unverified items: No items remain.

## Evidence

### Repository Evidence

- File: `remote file.txt:L1-L2`

### External Evidence

- Not applicable: test fixtures define the behavior.

### Performance Evidence

- Not applicable: no performance behavior changes.

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

const ISSUE_BODY: &str = r#"### Problem

Evidence can reference invalid head content.

### Desired observable outcome

Only valid head content passes.

### Acceptance criteria

- [ ] Validate remote evidence.

### Non-goals

- Judging evidence truth.

### Decisions needed

None

### Evidence plan

Use deterministic API fixtures.

### Applicable quality gates

- Tests, coverage, and mutation testing.

### Review scope

- Remote evidence policy tests.
"#;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-remote-evidence-test-{}-{id}",
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
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_repo-policy"))
}

fn run(environment: &[(&str, &str)]) -> Output {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new();
    let gh = directory.path().join("gh");
    fs::write(
        &gh,
        r###"#!/bin/sh
endpoint=$2
case "$endpoint" in
  repos/devdesignersid/buttter/issues/6)
    case "$4" in
      *pull_request*) printf 'open\n' ;;
      .body) printf '%s\n' "$ISSUE_BODY" ;;
      *implementation-approved*) printf 'true\n' ;;
      *) exit 90 ;;
    esac ;;
  repos/devdesignersid/buttter/issues/6/timeline)
    printf 'labeled\t2026-09-22T06:53:07Z\tdevdesignersid\n' ;;
  repos/devdesignersid/buttter/pulls/10)
    case "$4" in
      .created_at) printf '2026-09-22T06:53:08Z\n' ;;
      .head.sha)
        [ "${REMOVE_GH-}" = true ] && /bin/rm "$0"
        printf '%s\n' "${HEAD_SHA-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}"
        ;;
      *) exit 91 ;;
    esac ;;
  repos/devdesignersid/buttter/contents/remote%20file.txt\?ref=*)
    [ "${FAIL_RAW-}" = true ] && { printf 'raw failure\n' >&2; exit 1; }
    printf '%b' "${CONTENT-one\\ntwo\\n}" ;;
  *) printf 'unexpected endpoint: %s\n' "$endpoint" >&2; exit 92 ;;
esac
"###,
    )
    .unwrap();
    let mut permissions = fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gh, permissions).unwrap();

    let mut child = command()
        .args([
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("ISSUE_BODY", ISSUE_BODY)
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
        .write_all(BODY.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn assert_rejected(environment: &[(&str, &str)], diagnostic: &str) {
    let output = run(environment);
    assert!(!output.status.success(), "policy unexpectedly passed");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
        "missing `{diagnostic}` in {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn accepts_percent_encoded_head_file_evidence() {
    let output = run(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_invalid_head_content_and_api_failures() {
    assert_rejected(&[("HEAD_SHA", "aaaa")], "head revision");
    assert_rejected(
        &[("HEAD_SHA", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")],
        "head revision",
    );
    assert_rejected(&[("CONTENT", "one\\n")], "line range");
    assert_rejected(&[("CONTENT", "\\377")], "UTF-8");
    assert_rejected(&[("FAIL_RAW", "true")], "raw failure");
    assert_rejected(&[("REMOVE_GH", "true")], "could not start GitHub CLI");
}
