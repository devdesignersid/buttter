use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-commit-message-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
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

fn run_with_stdin(message: &[u8]) -> Output {
    let mut child = command()
        .arg("validate-commit-message")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(message).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_valid(message: &str) {
    let output = run_with_stdin(message.as_bytes());
    assert!(
        output.status.success(),
        "message `{message}` was rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Commit message is valid.\n");
    assert!(output.stderr.is_empty());
}

fn assert_invalid(message: &str, diagnostic: &str) {
    let output = run_with_stdin(message.as_bytes());
    assert!(!output.status.success(), "message `{message}` passed");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
        "missing `{diagnostic}` in {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn accepts_every_supported_type() {
    for commit_type in [
        "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style",
        "test",
    ] {
        assert_valid(&format!("{commit_type}: describe the change"));
    }
}

#[test]
fn accepts_scopes_breaking_markers_bodies_and_trailers() {
    for message in [
        "feat(parser): add commit parsing",
        "feat!: replace the policy interface",
        "feat(policy)!: replace the policy interface",
        "fix(api client): preserve scoped compatibility",
        "docs: explain the policy\n\nThis body has free-form text.\n\nIt can contain paragraphs.\nhttps://www.conventionalcommits.org/",
        "fix: close the defect\n\nReviewed-by: Example User\nRefs #10",
        "feat: change behavior\n\nBREAKING CHANGE: callers must pass a message",
        "feat: change behavior\n\nBREAKING-CHANGE: callers must pass a message",
        "feat: document a long trailer\n\nReviewed-by: Example User\n continuation line",
        "fix: accept Windows input\r\n\r\nA CRLF body.\r\n",
    ] {
        assert_valid(message);
    }
}

#[test]
fn exempts_git_generated_merge_messages() {
    for message in [
        "Merge branch 'main' into feat/policy",
        "Merge pull request #10 from devdesignersid/feat/policy\n\nAnything may follow.",
    ] {
        assert_valid(message);
    }
}

#[test]
fn validates_reverts_as_a_supported_conventional_type() {
    assert_valid("revert: remove accidental policy change");
    assert_invalid("Revert \"feat: accidental change\"", "header");
}

#[test]
fn rejects_malformed_headers_with_specific_diagnostics() {
    for (message, diagnostic) in [
        ("", "subject"),
        ("feature: add behavior", "unsupported type"),
        ("Feat: add behavior", "unsupported type"),
        ("feat add behavior", "header"),
        ("feat:no separating space", "header"),
        ("feat): add behavior", "scope"),
        ("feat:", "description"),
        ("feat:   ", "description"),
        ("feat(): add behavior", "scope"),
        ("feat((api)): add behavior", "scope"),
        ("feat(api: add behavior", "header"),
        ("feat: add behavior\nbody without a separator", "blank line"),
        ("feat: add behavior\rbody", "carriage return"),
    ] {
        assert_invalid(message, diagnostic);
    }
}

#[test]
fn rejects_malformed_trailers_with_specific_diagnostics() {
    for (message, diagnostic) in [
        ("feat: add behavior\n\nReviewed-by:", "trailer value"),
        (
            "feat: add behavior\n\nReviewed-by: Example User\nnot a continuation",
            "trailer",
        ),
        (
            "feat: add behavior\n\nBREAKING CHANGE:",
            "breaking-change trailer",
        ),
        ("feat: add behavior\n\nBad_Token: value", "trailer token"),
    ] {
        assert_invalid(message, diagnostic);
    }
}

#[test]
fn reads_a_message_file_and_fails_closed_for_input_errors() {
    let directory = TempDir::new();
    let valid = directory.0.join("valid-message");
    fs::write(&valid, "fix: validate a file\n").unwrap();
    let output = command()
        .args(["validate-commit-message", valid.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let missing = directory.0.join("missing-message");
    let output = command()
        .args(["validate-commit-message", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not read"));

    let invalid = directory.0.join("invalid-message");
    fs::write(&invalid, [0xff]).unwrap();
    let output = command()
        .args(["validate-commit-message", invalid.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UTF-8"));
}

#[test]
fn rejects_extra_commit_message_arguments() {
    let output = command()
        .args(["validate-commit-message", "one", "two"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}
