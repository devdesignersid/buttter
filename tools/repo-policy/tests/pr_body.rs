use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const VALID_BODY: &str = r#"## Related Issue

- Closes #6

## Acceptance Criteria

- [x] The policy rejects incomplete pull request bodies.

## Scope

### Included

- Pull request body validation.

### Excluded

- None: application development is not part of this change.

## Evidence

- Repository evidence: `AGENTS.md:9-24` and `.github/pull_request_template.md:1-40`.
- External evidence: Not applicable: the body rules are repository-defined.
- Performance evidence or not applicable: Not applicable: no performance behavior changes.

## Executed Quality Gates

| Gate or command | Result |
| --- | --- |
| `cargo test` | PASS |

## Human Line Review

- [x] The user has read every line of repository-authored changes.
- Generated files excluded from line review: `Cargo.lock`.
- Third-party source excluded from line review: None.
- Concerns requiring additional review: None.

## Unverified Items

- None
"#;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("repo-policy-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("create temporary test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temporary test directory");
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_repo-policy"))
}

fn run_with_stdin(args: &[&str], body: &str) -> Output {
    let mut child = command()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start repo-policy");
    child
        .stdin
        .take()
        .expect("open child stdin")
        .write_all(body.as_bytes())
        .expect("write body to child stdin");
    child.wait_with_output().expect("collect command output")
}

fn assert_invalid(body: &str, diagnostic: &str) {
    let output = run_with_stdin(&["validate-pr-body"], body);
    assert!(!output.status.success(), "body unexpectedly passed");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
        "missing diagnostic `{diagnostic}` in: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn replace_once(source: &str, from: &str, to: &str) -> String {
    assert_eq!(
        source.matches(from).count(),
        1,
        "test replacement must be unique"
    );
    source.replacen(from, to, 1)
}

#[test]
fn accepts_a_complete_body_from_standard_input() {
    let output = run_with_stdin(&["validate-pr-body"], VALID_BODY);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Pull request body is valid.\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn accepts_a_complete_body_from_a_file() {
    let directory = TempDir::new();
    let body_path = directory.path().join("body.md");
    fs::write(&body_path, VALID_BODY).unwrap();

    let output = command()
        .args(["validate-pr-body", body_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn handles_complete_and_unclosed_html_comments() {
    let body = VALID_BODY.replace(
        "## Acceptance Criteria\n",
        "## Acceptance Criteria\n\n<!-- Check every criterion before review. -->",
    );
    let output = run_with_stdin(&["validate-pr-body"], &body);
    assert!(output.status.success());

    let unclosed = VALID_BODY.replace("## Unverified Items", "<!--\n## Unverified Items");
    assert_invalid(&unclosed, "required headings");
}

#[test]
fn rejects_missing_duplicate_reordered_and_additional_headings() {
    assert_invalid(
        &VALID_BODY.replace("## Unverified Items\n", ""),
        "required headings",
    );
    assert_invalid(
        &VALID_BODY.replace(
            "## Unverified Items\n",
            "## Evidence\n\n## Unverified Items\n",
        ),
        "required headings",
    );
    assert_invalid(
        &VALID_BODY
            .replace("## Acceptance Criteria", "## Scope TEMP")
            .replace("## Scope\n", "## Acceptance Criteria\n")
            .replace("## Scope TEMP", "## Scope"),
        "required headings",
    );
    assert_invalid(
        &VALID_BODY.replace("## Evidence\n", "## Unexpected\n\n## Evidence\n"),
        "required headings",
    );
}

#[test]
fn rejects_missing_malformed_zero_and_multiline_related_issues() {
    assert_invalid(
        &replace_once(VALID_BODY, "- Closes #6", "- Closes #"),
        "Related Issue",
    );
    assert_invalid(
        &replace_once(VALID_BODY, "- Closes #6", "- Relates to #6"),
        "Related Issue",
    );
    assert_invalid(
        &replace_once(VALID_BODY, "- Closes #6", "- Closes #0"),
        "Related Issue",
    );
    assert_invalid(
        &replace_once(VALID_BODY, "- Closes #6", "- Closes #6\n- Closes #7"),
        "Related Issue",
    );
}

#[test]
fn rejects_missing_unchecked_empty_and_malformed_acceptance_criteria() {
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- [x] The policy rejects incomplete pull request bodies.",
            "- [ ]",
        ),
        "Acceptance Criteria",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- [x] The policy rejects incomplete pull request bodies.",
            "- [ ] The policy rejects incomplete pull request bodies.",
        ),
        "Acceptance Criteria",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- [x] The policy rejects incomplete pull request bodies.",
            "The policy rejects incomplete pull request bodies.",
        ),
        "Acceptance Criteria",
    );
}

#[test]
fn accepts_multiple_checked_acceptance_criteria() {
    let body = replace_once(
        VALID_BODY,
        "- [x] The policy rejects incomplete pull request bodies.",
        "- [x] The policy rejects incomplete pull request bodies.\n- [X] The policy reports a diagnostic.",
    );
    assert!(
        run_with_stdin(&["validate-pr-body"], &body)
            .status
            .success()
    );
}

#[test]
fn rejects_empty_or_malformed_scope_lists() {
    assert_invalid(
        &replace_once(VALID_BODY, "- Pull request body validation.", "-"),
        "Included",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- None: application development is not part of this change.",
            "application development",
        ),
        "Excluded",
    );
}

#[test]
fn rejects_every_empty_evidence_field_and_unexpected_evidence_lines() {
    for value in [
        "`AGENTS.md:9-24` and `.github/pull_request_template.md:1-40`.",
        "Not applicable: the body rules are repository-defined.",
        "Not applicable: no performance behavior changes.",
    ] {
        assert_invalid(&replace_once(VALID_BODY, value, ""), "Evidence");
    }
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- External evidence: Not applicable: the body rules are repository-defined.",
            "External evidence exists.",
        ),
        "Evidence",
    );
}

#[test]
fn rejects_empty_malformed_and_unsuccessful_quality_gate_rows() {
    assert_invalid(
        &replace_once(VALID_BODY, "| `cargo test` | PASS |", "|  |  |"),
        "Executed Quality Gates",
    );
    assert_invalid(
        &replace_once(VALID_BODY, "| `cargo test` | PASS |", "cargo test: PASS"),
        "Executed Quality Gates",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "| `cargo test` | PASS |",
            "| `cargo test` | FAIL |",
        ),
        "Executed Quality Gates",
    );
    assert_invalid(
        &replace_once(VALID_BODY, "| `cargo test` | PASS |", "|  | PASS |"),
        "Executed Quality Gates",
    );
}

#[test]
fn accepts_multiple_passing_quality_gates() {
    let body = replace_once(
        VALID_BODY,
        "| `cargo test` | PASS |",
        "| `cargo test` | PASS |\n| `cargo fmt --check` | PASS |",
    );
    assert!(
        run_with_stdin(&["validate-pr-body"], &body)
            .status
            .success()
    );
}

#[test]
fn rejects_unchecked_human_review_and_every_empty_review_field() {
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "- [x] The user has read every line of repository-authored changes.",
            "- [ ] The user has read every line of repository-authored changes.",
        ),
        "Human Line Review",
    );
    for line in [
        "- Generated files excluded from line review: `Cargo.lock`.",
        "- Third-party source excluded from line review: None.",
        "- Concerns requiring additional review: None.",
    ] {
        let label = line.split_once(':').unwrap().0;
        let body = replace_once(VALID_BODY, line, &format!("{label}:"));
        assert_invalid(&body, "Human Line Review");
    }
}

#[test]
fn rejects_empty_and_malformed_unverified_items() {
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "## Unverified Items\n\n- None",
            "## Unverified Items\n\n-",
        ),
        "Unverified Items",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            "## Unverified Items\n\n- None",
            "## Unverified Items\n\nNone",
        ),
        "Unverified Items",
    );
}

#[test]
fn reports_all_independent_body_errors_together() {
    let body = replace_once(VALID_BODY, "- Closes #6", "- Closes #");
    let body = replace_once(&body, "| `cargo test` | PASS |", "|  |  |");
    let output = run_with_stdin(&["validate-pr-body"], &body);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success());
    assert!(stderr.contains("Related Issue"));
    assert!(stderr.contains("Executed Quality Gates"));
}

#[test]
fn rejects_missing_unreadable_and_non_utf8_body_files() {
    let directory = TempDir::new();
    let missing = directory.path().join("missing.md");
    let missing_output = command()
        .args(["validate-pr-body", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!missing_output.status.success());
    assert!(String::from_utf8_lossy(&missing_output.stderr).contains("could not read"));

    let invalid = directory.path().join("invalid.md");
    fs::write(&invalid, [0xff]).unwrap();
    let invalid_output = command()
        .args(["validate-pr-body", invalid.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!invalid_output.status.success());
    assert!(String::from_utf8_lossy(&invalid_output.stderr).contains("UTF-8"));
}

#[cfg(unix)]
#[test]
fn reports_standard_input_read_failures() {
    let directory = TempDir::new();
    let input = fs::File::open(directory.path()).unwrap();
    let output = command()
        .arg("validate-pr-body")
        .stdin(Stdio::from(input))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not read standard input"));
}

#[test]
fn rejects_unknown_commands_arguments_and_invalid_repository_names() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["validate-pr-body", "one", "two"],
        vec!["validate-pr-body", "--github-repository"],
        vec!["validate-pr-body", "--github-repository", "invalid"],
        vec![
            "validate-pr-body",
            "--github-repository",
            "owner/repo/extra",
        ],
    ] {
        let output = run_with_stdin(&args, VALID_BODY);
        assert!(
            !output.status.success(),
            "arguments unexpectedly passed: {args:?}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}

#[test]
fn prints_help_without_reading_a_body() {
    for argument in ["--help", "-h"] {
        let output = command().arg(argument).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        for expected in [
            "Usage:",
            "Commands:",
            "--github-repository",
            "standard input",
            "Exit status:",
        ] {
            assert!(stdout.contains(expected), "help is missing `{expected}`");
        }
        assert!(output.stderr.is_empty());
    }
}

#[cfg(unix)]
fn fake_gh(script_body: &str) -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new();
    let gh_path = directory.path().join("gh");
    fs::write(&gh_path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    let mut permissions = fs::metadata(&gh_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(gh_path, permissions).unwrap();
    directory
}

#[cfg(unix)]
fn run_with_fake_gh(script_body: &str) -> Output {
    let directory = fake_gh(script_body);
    let mut child = command()
        .args([
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
        ])
        .env("PATH", directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(VALID_BODY.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn verifies_an_open_issue_through_github_cli() {
    let output = run_with_fake_gh(
        "test \"$1\" = api || exit 90\n\
         test \"$2\" = repos/devdesignersid/buttter/issues/6 || exit 91\n\
         test \"$3\" = --jq || exit 92\n\
         printf 'open\\n'",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn rejects_closed_issues_pull_requests_and_unexpected_api_responses() {
    for (response, diagnostic) in [
        ("closed", "not open"),
        ("pull_request", "pull request"),
        ("unexpected", "unexpected response"),
    ] {
        let output = run_with_fake_gh(&format!("printf '{response}\\n'"));
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "missing `{diagnostic}` in {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(unix)]
#[test]
fn reports_github_cli_api_and_utf8_failures() {
    let failed = run_with_fake_gh("printf 'API unavailable\\n' >&2\nexit 1");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("API unavailable"));

    let invalid_utf8 = run_with_fake_gh("printf '\\377'");
    assert!(!invalid_utf8.status.success());
    assert!(String::from_utf8_lossy(&invalid_utf8.stderr).contains("UTF-8"));

    let directory = TempDir::new();
    let mut child = command()
        .args([
            "validate-pr-body",
            "--github-repository",
            "devdesignersid/buttter",
        ])
        .env("PATH", directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(VALID_BODY.as_bytes())
        .unwrap();
    let missing = child.wait_with_output().unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("could not start GitHub CLI"));
}
