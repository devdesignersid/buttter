#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-commit-hook-test-{}-{id}",
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

fn run_hook(validator_status: i32) -> (std::process::Output, String, TempDir) {
    let directory = TempDir::new();
    let cargo = directory.0.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE\"\nexit \"$VALIDATOR_STATUS\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cargo, permissions).unwrap();

    let message = directory.0.join("COMMIT_EDITMSG");
    fs::write(&message, "feat: test the hook\n").unwrap();
    let capture = directory.0.join("arguments");
    let hook = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.githooks/commit-msg");
    let path = format!(
        "{}:{}",
        directory.0.display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new("sh")
        .arg(hook)
        .arg(&message)
        .env("PATH", path)
        .env("CAPTURE", &capture)
        .env("VALIDATOR_STATUS", validator_status.to_string())
        .output()
        .unwrap();
    let arguments = fs::read_to_string(capture).unwrap_or_default();
    (output, arguments, directory)
}

#[test]
fn passes_the_message_file_to_the_repository_policy_cli() {
    let (output, arguments, _directory) = run_hook(0);
    assert!(output.status.success());
    assert!(arguments.contains("--manifest-path"));
    assert!(arguments.contains("tools/repo-policy/Cargo.toml"));
    assert!(arguments.contains("validate-commit-message"));
    assert!(arguments.contains("COMMIT_EDITMSG"));
}

#[test]
fn rejects_the_commit_when_the_validator_fails() {
    let (output, _arguments, _directory) = run_hook(1);
    assert!(!output.status.success());
}
