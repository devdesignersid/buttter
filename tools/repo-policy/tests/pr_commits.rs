#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-pr-commits-test-{}-{id}",
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

fn record(sha: &str, author: &str, message: &str) -> String {
    format!(
        "{}\t{}\t{}\n",
        percent_encode(sha),
        percent_encode(author),
        percent_encode(message)
    )
}

fn fake_gh() -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new();
    let path = directory.path().join("gh");
    fs::write(
        &path,
        r#"#!/bin/sh
[ "$1" = api ] || { printf 'expected api command\n' >&2; exit 90; }
found_paginate=false
found_query=false
for argument in "$@"; do
  [ "$argument" = --paginate ] && found_paginate=true
  [ "$argument" = --jq ] && found_query=true
done
[ "$found_query" = true ] || { printf 'request did not select data\n' >&2; exit 91; }
if [ -n "${GH_FAILURE-}" ] && { [ -z "${GH_FAILURE_ENDPOINT-}" ] || [ "$GH_FAILURE_ENDPOINT" = "$2" ]; }; then
  printf '%s\n' "$GH_FAILURE" >&2
  exit 1
fi
case "$2" in
  repos/devdesignersid/buttter/pulls/10)
    [ "$found_paginate" = false ] || { printf 'count request was paginated\n' >&2; exit 92; }
    printf '%s\n' "$COMMIT_COUNT"
    ;;
  repos/devdesignersid/buttter/pulls/10/commits)
    [ "$found_paginate" = true ] || { printf 'commit request was not paginated\n' >&2; exit 93; }
    printf '%b' "$COMMIT_RECORDS"
    ;;
  *) printf 'unexpected endpoint\n' >&2; exit 94 ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
    directory
}

fn run_with_count(records: &str, count: &str) -> Output {
    let directory = fake_gh();
    command()
        .args([
            "validate-pr-commits",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("COMMIT_COUNT", count)
        .env("COMMIT_RECORDS", records)
        .output()
        .unwrap()
}

fn run(records: &str) -> Output {
    run_with_count(records, &records.lines().count().to_string())
}

#[test]
fn validates_every_pull_request_commit() {
    let records = format!(
        "{}{}{}",
        record(
            "1111111111111111111111111111111111111111",
            "contributor",
            "feat: first change"
        ),
        record(
            "2222222222222222222222222222222222222222",
            "contributor",
            "fix(api): second change"
        ),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tcontributor\tdocs%3a%20third%20change\n"
    );
    let output = run(&records);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Pull request commit messages are valid.\n");
}

#[test]
fn reports_the_sha_for_each_invalid_commit() {
    let sha = "3333333333333333333333333333333333333333";
    let output = run(&record(sha, "contributor", "invalid message"));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(sha));
    assert!(stderr.contains("header"));
}

#[test]
fn exempts_verified_github_bot_and_merge_commits() {
    let records = format!(
        "{}{}",
        record(
            "4444444444444444444444444444444444444444",
            "dependabot[bot]",
            "Bump a dependency"
        ),
        record(
            "5555555555555555555555555555555555555555",
            "contributor",
            "Merge branch 'main'"
        )
    );
    assert!(run(&records).status.success());
}

#[test]
fn does_not_trust_unverified_or_lookalike_bot_authors() {
    for author in ["", "dependabot", "dependabot[bot]extra", "[bot]"] {
        let output = run(&record(
            "6666666666666666666666666666666666666666",
            author,
            "Bump a dependency",
        ));
        assert!(!output.status.success(), "author `{author}` was trusted");
    }
}

#[test]
fn fails_closed_for_api_and_commit_data_errors() {
    let directory = fake_gh();
    let api_failure = command()
        .args([
            "validate-pr-commits",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("COMMIT_COUNT", "1")
        .env("GH_FAILURE", "simulated commit API failure")
        .output()
        .unwrap();
    assert!(!api_failure.status.success());
    assert!(String::from_utf8_lossy(&api_failure.stderr).contains("simulated commit API failure"));

    let list_failure = command()
        .args([
            "validate-pr-commits",
            "--github-repository",
            "devdesignersid/buttter",
            "--pull-request-number",
            "10",
        ])
        .env("PATH", directory.path())
        .env("COMMIT_COUNT", "1")
        .env("GH_FAILURE", "simulated commit-list API failure")
        .env(
            "GH_FAILURE_ENDPOINT",
            "repos/devdesignersid/buttter/pulls/10/commits",
        )
        .output()
        .unwrap();
    assert!(!list_failure.status.success());
    assert!(
        String::from_utf8_lossy(&list_failure.stderr).contains("simulated commit-list API failure")
    );

    let empty = run_with_count("", "1");
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("commit data"));

    for records in [
        "not-a-record\n",
        "short\tcontributor\tfeat%3A%20change\n",
        "7777777777777777777777777777777777777777\tcontributor\tbad%\n",
        "7777777777777777777777777777777777777777\tcontributor\tbad%XXdata\n",
        "7777777777777777777777777777777777777777\tcontributor\tbad%AXdata\n",
        "7777777777777777777777777777777777777777\tcontributor\t%FF\n",
    ] {
        let output = run(records);
        assert!(
            !output.status.success(),
            "records unexpectedly passed: {records}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("commit data"));
    }

    let valid = record(
        "8888888888888888888888888888888888888888",
        "contributor",
        "fix: valid message",
    );
    for count in ["0", "invalid", "251", "2"] {
        let output = run_with_count(&valid, count);
        assert!(
            !output.status.success(),
            "count `{count}` unexpectedly passed"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("commit data"));
    }

    let duplicate = format!("{valid}{valid}");
    let output = run_with_count(&duplicate, "2");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("duplicate"));
}

#[test]
fn requires_complete_pull_request_arguments() {
    for args in [
        vec!["validate-pr-commits"],
        vec![
            "validate-pr-commits",
            "--github-repository",
            "devdesignersid/buttter",
        ],
        vec!["validate-pr-commits", "--pull-request-number", "10"],
        vec![
            "validate-pr-commits",
            "--github-repository",
            "invalid",
            "--pull-request-number",
            "10",
        ],
    ] {
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}
