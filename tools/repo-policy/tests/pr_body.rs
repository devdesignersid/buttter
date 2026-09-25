use std::fs;
use std::io::{ErrorKind, Write};
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

## Pre-Completion Review

- [x] Requirement fit: Every acceptance criterion is implemented and verified.
- [x] Boundaries and regressions: The validator changes apply only to pull request bodies.
- [x] Design: The existing deterministic parser remains responsible for body validation.
- [x] Tests: Valid, boundary, and malformed records are covered.
- [x] Correctness: Field order and record syntax were inspected.
- [x] Standards: Repository formatting, lint, coverage, and mutation gates pass.
- [x] Scope: The diff contains only approved policy files.
- [x] Maintainability: Record formats are documented beside the validator.
- [x] Side effects: Invalid pull request bodies now fail the existing required check.
- [x] Documentation: The template and policy contract are updated together.
- [x] Evidence: Repository, external, and performance records follow below.
- [x] Unverified items: No items remain unverified.

## Evidence

### Repository Evidence

- File: `src/main.rs:L1-L10`
- Command: `cargo test --manifest-path tools/repo-policy/Cargo.toml` | Output: `test result: ok`

### External Evidence

- Not applicable: the behavior is repository-defined.

### Performance Evidence

- Not applicable: no performance behavior changes.

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
    run_with_stdin_in(args, body, None)
}

fn run_with_stdin_in(args: &[&str], body: &str, directory: Option<&Path>) -> Output {
    let mut command = command();
    command.args(args);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start repo-policy");
    if let Err(error) = child
        .stdin
        .take()
        .expect("open child stdin")
        .write_all(body.as_bytes())
    {
        assert_eq!(
            error.kind(),
            ErrorKind::BrokenPipe,
            "write body to child stdin: {error}"
        );
    }
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
fn rejects_unchecked_empty_placeholder_duplicate_and_reordered_review_fields() {
    let requirement =
        "- [x] Requirement fit: Every acceptance criterion is implemented and verified.";
    assert_invalid(
        &replace_once(VALID_BODY, requirement, &requirement.replace("[x]", "[ ]")),
        "Requirement fit",
    );
    assert_invalid(
        &replace_once(VALID_BODY, requirement, "- [x] Requirement fit: TODO"),
        "Requirement fit",
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            requirement,
            "- [x] Requirement fit: <replace this>",
        ),
        "Requirement fit",
    );
    let comparison = replace_once(
        VALID_BODY,
        requirement,
        "- [x] Requirement fit: The comparison operator is >",
    );
    assert!(
        run_with_stdin(&["validate-pr-body"], &comparison)
            .status
            .success()
    );
    assert_invalid(
        &replace_once(
            VALID_BODY,
            requirement,
            &format!("{requirement}\n{requirement}"),
        ),
        "Requirement fit",
    );
    let reordered = VALID_BODY
        .replace("- [x] Requirement fit:", "- [x] Design TEMP:")
        .replace("- [x] Design:", "- [x] Requirement fit:")
        .replace("- [x] Design TEMP:", "- [x] Design:");
    assert_invalid(&reordered, "Pre-Completion Review");

    for label in [
        "Boundaries and regressions",
        "Design",
        "Tests",
        "Correctness",
        "Standards",
        "Scope",
        "Maintainability",
        "Side effects",
        "Documentation",
        "Evidence",
        "Unverified items",
    ] {
        let prefix = format!("- [x] {label}:");
        let line = VALID_BODY
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap();
        assert_invalid(&replace_once(VALID_BODY, line, &prefix), label);
    }
}

#[test]
fn validates_repository_file_and_command_evidence() {
    for (value, diagnostic) in [
        ("`src/main.rs:L1-L10`", "Repository Evidence"),
        ("- File: `src/main.rs:L0-L10`", "line range"),
        ("- File: `src/main.rs:L10-L1`", "line range"),
        ("- File: `src/main.rs:L1-L9999`", "line range"),
        ("- File: src/main.rs:L1-L10", "file records must use"),
        ("- File: `src/main.rs`", "line range"),
        ("- File: `src/main.rs:L1-10`", "line range"),
        ("- File: `src/main.rs:Lx-L10`", "line range"),
        ("- File: `src/main.rs:L1-Lx`", "line range"),
        ("- File: `missing.md:L1-L2`", "does not exist"),
        ("- File: `../AGENTS.md:L1-L2`", "repository-relative"),
        ("- File: `/AGENTS.md:L1-L2`", "repository-relative"),
        ("- Command: cargo test", "documented syntax"),
        ("- Command: `cargo test`", "include ` | Output: `"),
        (
            "- Command: `cargo test` | Output: `ok",
            "end with a backtick",
        ),
        ("- Command: `` | Output: `ok`", "command"),
        ("- Command: `cargo test` | Output: ``", "output"),
        ("- Unexpected evidence", "Repository Evidence"),
    ] {
        assert_invalid(
            &replace_once(VALID_BODY, "- File: `src/main.rs:L1-L10`", value),
            diagnostic,
        );
    }

    let empty = VALID_BODY.replace(
        "- File: `src/main.rs:L1-L10`\n- Command: `cargo test --manifest-path tools/repo-policy/Cargo.toml` | Output: `test result: ok`",
        "",
    );
    assert_invalid(&empty, "Repository Evidence must contain");
}

#[cfg(unix)]
#[test]
fn validates_local_repository_file_types_boundaries_and_failures() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = TempDir::new();
    fs::create_dir(directory.path().join("folder")).unwrap();
    fs::write(directory.path().join("short.txt"), "one\ntwo\n").unwrap();
    symlink("/etc/hosts", directory.path().join("outside.txt")).unwrap();
    let unreadable = directory.path().join("unreadable.txt");
    fs::write(&unreadable, "one\ntwo\n").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    for (path, diagnostic) in [
        ("folder", "repository file"),
        ("outside.txt", "repository file"),
        ("unreadable.txt", "could not read"),
        ("short.txt", "line range"),
    ] {
        let body = replace_once(VALID_BODY, "src/main.rs:L1-L10", &format!("{path}:L1-L10"));
        let output = run_with_stdin_in(&["validate-pr-body"], &body, Some(directory.path()));
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "missing `{diagnostic}` in {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o600)).unwrap();

    let boundary = replace_once(VALID_BODY, "src/main.rs:L1-L10", "short.txt:L1-L2");
    let output = run_with_stdin_in(&["validate-pr-body"], &boundary, Some(directory.path()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let deleted = TempDir::new();
    let mut child = command()
        .arg("validate-pr-body")
        .current_dir(deleted.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fs::remove_dir(deleted.path()).unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(VALID_BODY.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    std::mem::forget(deleted);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("repository root"));
}

#[test]
fn validates_external_evidence_records_and_dates() {
    let not_applicable = "- Not applicable: the behavior is repository-defined.";
    let valid = "- Source: https://doc.rust-lang.org/reference/ | Version/revision: Rust 1.98.1 | Accessed: 2025-02-28";
    let body = replace_once(VALID_BODY, not_applicable, valid);
    let output = run_with_stdin(&["validate-pr-body"], &body);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for value in [
        "- Source: http://example.com | Version/revision: v1 | Accessed: 2025-02-28",
        "- Source: https:///example.com | Version/revision: v1 | Accessed: 2025-02-28",
        "- Source: https://localhost/source | Version/revision: v1 | Accessed: 2025-02-28",
        "- Source: https://example.com | Version/revision:  | Accessed: 2025-02-28",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2025-02-29",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 1900-02-29",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2024-04-31",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 0000-01-01",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2025-13-01",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2025-01-aa",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2025-1-01",
        "- Source: https://example.com | Version/revision: v1 | Accessed: 2025-01",
        "- Not applicable:",
    ] {
        assert_invalid(
            &replace_once(VALID_BODY, not_applicable, value),
            "External Evidence",
        );
    }

    let empty = VALID_BODY.replace("- Not applicable: the behavior is repository-defined.", "");
    assert_invalid(&empty, "External Evidence must contain");

    for date in ["2024-02-29", "2000-02-29", "2025-04-30", "2025-01-31"] {
        let body = replace_once(
            VALID_BODY,
            "- Not applicable: the behavior is repository-defined.",
            &format!(
                "- Source: https://example.com/source | Version/revision: v1 | Accessed: {date}"
            ),
        );
        assert!(
            run_with_stdin(&["validate-pr-body"], &body)
                .status
                .success()
        );
    }
}

#[test]
fn validates_performance_evidence_or_reason() {
    let not_applicable = "- Not applicable: no performance behavior changes.";
    let command = "- Command: `hyperfine './validator fixture.md'` | Output: `median 12.4 ms`";
    let body = replace_once(VALID_BODY, not_applicable, command);
    assert!(
        run_with_stdin(&["validate-pr-body"], &body)
            .status
            .success()
    );

    for value in [
        "",
        "- Not applicable:",
        "- Command: `` | Output: `median 12.4 ms`",
        "- Command: `hyperfine validator` | Output: ``",
        "- Result: fast",
        "- Not applicable: no performance change.\n- Not applicable: no timing applies.",
    ] {
        assert_invalid(
            &replace_once(VALID_BODY, not_applicable, value),
            "Performance Evidence",
        );
    }
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
fn rejects_empty_malformed_and_mixed_unverified_items() {
    for value in ["-", "None", "- TODO", "- None\n- A concrete item"] {
        assert_invalid(
            &replace_once(
                VALID_BODY,
                "## Unverified Items\n\n- None",
                &format!("## Unverified Items\n\n{value}"),
            ),
            "Unverified Items",
        );
    }

    let concrete = replace_once(
        VALID_BODY,
        "## Unverified Items\n\n- None",
        "## Unverified Items\n\n- The production ruleset cannot be inspected offline.",
    );
    assert!(
        run_with_stdin(&["validate-pr-body"], &concrete)
            .status
            .success()
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
