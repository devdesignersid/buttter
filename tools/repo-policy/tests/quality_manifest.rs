#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repo-policy-quality-test-{}-{id}",
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

fn run(root: &Path, arguments: &[&str]) -> Output {
    command()
        .args(arguments)
        .arg("--repository-root")
        .arg(root)
        .output()
        .unwrap()
}

fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn manifest(platform: &str) -> String {
    format!(
        r#"version = 1

[tools]
rust = "1.98.1"
cargo_llvm_cov = "0.9.1"

[[targets]]
name = "app"
kind = "rust"
platform = "{platform}"
cargo_manifest = "app/Cargo.toml"
product_sources = ["app/src"]
test_sources = ["app/tests"]
generated = ["app/generated"]
third_party = ["app/vendor"]
supporting_files = ["scripts/hook"]
"#
    )
}

fn fixture(platform: &str) -> TempDir {
    let directory = TempDir::new();
    write(directory.path().join("quality.toml"), manifest(platform));
    write(
        directory.path().join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        directory.path().join("app/src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    );
    write(directory.path().join("app/src/readme.txt"), "not source\n");
    write(
        directory.path().join("app/tests/answer.rs"),
        "#[test]\nfn answer() {}\n",
    );
    write(
        directory.path().join("app/generated/bindings.rs"),
        "generated!();\n",
    );
    write(
        directory.path().join("app/vendor/dependency.rs"),
        "third_party!();\n",
    );
    let hook = directory.path().join("scripts/hook");
    write(&hook, "#!/bin/sh\nexit 0\n");
    let mut permissions = fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).unwrap();

    Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(directory.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(directory.path())
        .status()
        .unwrap();
    directory
}

fn assert_failure(output: Output, diagnostic: &str) {
    assert!(!output.status.success(), "command unexpectedly passed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(diagnostic),
        "missing `{diagnostic}` in {stderr}"
    );
}

#[test]
fn validates_manifests_and_emits_platform_matrix() {
    let linux = fixture("linux");
    let output = run(linux.path(), &["validate-quality-manifest", "quality.toml"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Quality manifest is valid.\n");

    let output = command()
        .args(["validate-quality-manifest", "quality.toml"])
        .current_dir(linux.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = run(linux.path(), &["quality-matrix", "quality.toml"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        r#"{"include":[{"name":"app","runner":"ubuntu-24.04"}]}"#
    );

    write(linux.path().join("quality.toml"), manifest("macos"));
    let output = run(linux.path(), &["quality-matrix", "quality.toml"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        r#"{"include":[{"name":"app","runner":"macos-15"}]}"#
    );
}

#[test]
fn rejects_malformed_or_ambiguous_manifests() {
    for (contents, diagnostic) in [
        ("not toml", "TOML"),
        (&manifest("windows"), "platform"),
        (
            &manifest("linux").replace("version = 1", "version = 2"),
            "version",
        ),
        (
            &manifest("linux").replace("name = \"app\"", "name = \"app\"\nunknown = true"),
            "unknown",
        ),
        (
            &format!("{}\n{}", manifest("linux"), manifest("linux")),
            "duplicate",
        ),
        (
            &manifest("linux").replace("product_sources = [\"app/src\"]", "product_sources = []"),
            "product_sources",
        ),
        (
            &manifest("linux").replace("app/src", "../outside"),
            "repository-relative",
        ),
        (
            &manifest("linux").replace(
                "test_sources = [\"app/tests\"]",
                "test_sources = [\"app/src\"]",
            ),
            "overlap",
        ),
        (
            &manifest("linux").replace("rust = \"1.98.1\"", "rust = \"stable\""),
            "exact numeric version",
        ),
        (
            &manifest("linux").replace("name = \"app\"", "name = \"App\""),
            "invalid quality target name",
        ),
        (
            &manifest("linux").replace("kind = \"rust\"", "kind = \"swift\""),
            "only rust is permitted",
        ),
        (
            &manifest("linux").replace("test_sources = [\"app/tests\"]", "test_sources = []"),
            "test_sources",
        ),
        (
            &manifest("linux").replace(
                "generated = [\"app/generated\"]",
                "generated = [\"app/vendor\", \"app/generated\"]",
            ),
            "unique and sorted",
        ),
        (
            &manifest("linux").replace("app/Cargo.toml", "app/missing.toml"),
            "does not exist",
        ),
    ] {
        let directory = fixture("linux");
        write(directory.path().join("quality.toml"), contents);
        let output = run(
            directory.path(),
            &["validate-quality-manifest", "quality.toml"],
        );
        assert_failure(output, diagnostic);
    }
}

#[test]
fn rejects_unowned_rust_sources_and_executables() {
    for (path, contents, executable) in [
        ("unowned.rs", "fn hidden() {}\n", false),
        ("scripts/unowned", "#!/bin/sh\nexit 0\n", true),
    ] {
        let directory = fixture("linux");
        let path = directory.path().join(path);
        write(&path, contents);
        if executable {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
        }
        Command::new("git")
            .args(["add", "."])
            .current_dir(directory.path())
            .status()
            .unwrap();

        let output = run(
            directory.path(),
            &["validate-quality-manifest", "quality.toml"],
        );
        assert_failure(output, "not owned by a quality target");
    }
}

#[test]
fn fails_closed_for_git_ownership_errors_and_malformed_metadata() {
    let directory = fixture("linux");
    let empty_path = directory.path().join("no-git");
    fs::create_dir(&empty_path).unwrap();
    let output = command()
        .args([
            "validate-quality-manifest",
            "quality.toml",
            "--repository-root",
        ])
        .arg(directory.path())
        .env("PATH", &empty_path)
        .output()
        .unwrap();
    assert_failure(output, "could not run git to inspect source ownership");

    for (script, diagnostic) in [
        (
            "#!/bin/sh\necho denied >&2\nexit 1\n",
            "git could not inspect",
        ),
        ("#!/bin/sh\nprintf '\\377\\0'\n", "non-UTF-8 tracked path"),
        (
            "#!/bin/sh\nprintf 'malformed\\0'\n",
            "malformed tracked-file metadata",
        ),
        (
            "#!/bin/sh\nprintf '\\tfile.rs\\0'\n",
            "malformed tracked-file mode",
        ),
    ] {
        let directory = fixture("linux");
        let bin = directory.path().join("git-bin");
        fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        write(&git, script);
        let mut permissions = fs::metadata(&git).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&git, permissions).unwrap();
        let output = command()
            .args([
                "validate-quality-manifest",
                "quality.toml",
                "--repository-root",
            ])
            .arg(directory.path())
            .env("PATH", &bin)
            .output()
            .unwrap();
        assert_failure(output, diagnostic);
    }
}

fn coverage_report(root: &Path, count: u64) -> String {
    format!(
        "TN:\nSF:{}/app/src/lib.rs\nDA:1,{count}\nend_of_record\n",
        root.display()
    )
}

#[test]
fn accepts_complete_product_coverage_and_ignores_approved_non_product_sources() {
    let directory = fixture("linux");
    let report = directory.path().join("coverage.lcov");
    write(&report, coverage_report(directory.path(), 1));

    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Coverage for app is 100%.\n");
}

#[test]
fn rejects_uncovered_omitted_unknown_and_malformed_coverage_data() {
    for (report, diagnostic) in [
        ("uncovered", "uncovered line"),
        ("omitted", "missing product source"),
        ("unknown", "outside product sources"),
        ("bad-data", "invalid line data"),
        ("bad-line", "invalid line number"),
        ("bad-count", "invalid execution count"),
        ("zero-line", "line number must be positive"),
        ("line-without-source", "line without source"),
        ("end-without-source", "record has no source"),
        ("no-line-data", "has no line data"),
        ("unterminated", "unterminated source record"),
        ("duplicate", "duplicate source"),
        ("nested-source", "invalid source record"),
        ("missing-source", "source does not exist"),
        ("unknown-record", "unknown record"),
    ] {
        let directory = fixture("linux");
        let source = format!("{}/app/src/lib.rs", directory.path().display());
        let contents = match report {
            "uncovered" => coverage_report(directory.path(), 0),
            "omitted" => "TN:\n".to_owned(),
            "unknown" => format!(
                "SF:{}/app/tests/answer.rs\nDA:1,1\nend_of_record\n",
                directory.path().display()
            ),
            "bad-data" => format!("SF:{source}\nDA:1\nend_of_record\n"),
            "bad-line" => format!("SF:{source}\nDA:x,1\nend_of_record\n"),
            "bad-count" => format!("SF:{source}\nDA:1,x\nend_of_record\n"),
            "zero-line" => format!("SF:{source}\nDA:0,1\nend_of_record\n"),
            "line-without-source" => "DA:1,1\n".to_owned(),
            "end-without-source" => "end_of_record\n".to_owned(),
            "no-line-data" => format!("SF:{source}\nend_of_record\n"),
            "unterminated" => format!("SF:{source}\nDA:1,1\n"),
            "duplicate" => {
                format!("SF:{source}\nDA:1,1\nend_of_record\nSF:{source}\nDA:1,1\nend_of_record\n")
            }
            "nested-source" => format!("SF:{source}\nSF:{source}\n"),
            "missing-source" => format!("SF:{}/missing.rs\n", directory.path().display()),
            _ => format!("SF:{source}\nUNKNOWN\n"),
        };
        write(directory.path().join("coverage.lcov"), contents);
        let output = run(
            directory.path(),
            &[
                "validate-coverage-report",
                "quality.toml",
                "app",
                "coverage.lcov",
            ],
        );
        assert_failure(output, diagnostic);
    }

    let directory = fixture("linux");
    write(
        directory.path().join("coverage.lcov"),
        coverage_report(directory.path(), 1),
    );
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "missing",
            "coverage.lcov",
        ],
    );
    assert_failure(output, "unknown quality target");

    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "missing.lcov",
        ],
    );
    assert_failure(output, "could not read coverage report");

    write(directory.path().join("quality.toml"), "not toml");
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert_failure(output, "TOML");
}

#[test]
fn accepts_relative_coverage_sources_and_product_file_roots() {
    let directory = fixture("linux");
    write(
        directory.path().join("quality.toml"),
        manifest("linux").replace(
            "product_sources = [\"app/src\"]",
            "product_sources = [\"app/src/lib.rs\"]",
        ),
    );
    write(
        directory.path().join("coverage.lcov"),
        "SF:app/src/lib.rs\nDA:1,1\nend_of_record\n",
    );
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert!(output.status.success());
}

#[test]
fn rejects_product_roots_without_rust_and_symbolic_linked_sources() {
    let directory = fixture("linux");
    write(directory.path().join("app/empty/readme.txt"), "not Rust\n");
    write(
        directory.path().join("quality.toml"),
        manifest("linux").replace("app/src", "app/empty"),
    );
    write(directory.path().join("coverage.lcov"), "TN:\n");
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert_failure(output, "no Rust source files");

    write(directory.path().join("quality.toml"), manifest("linux"));
    let link = directory.path().join("app/src/link.rs");
    std::os::unix::fs::symlink(directory.path().join("app/src/lib.rs"), &link).unwrap();
    Command::new("git")
        .args(["add", "app/src/link.rs"])
        .current_dir(directory.path())
        .status()
        .unwrap();
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert_failure(output, "symbolic link");
}

#[test]
fn fails_closed_when_git_cannot_list_product_sources() {
    let directory = fixture("linux");
    write(
        directory.path().join("coverage.lcov"),
        coverage_report(directory.path(), 1),
    );
    let empty_path = directory.path().join("no-product-git");
    fs::create_dir(&empty_path).unwrap();
    let output = command()
        .args([
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
            "--repository-root",
        ])
        .arg(directory.path())
        .env("PATH", &empty_path)
        .output()
        .unwrap();
    assert_failure(output, "could not run git to list product sources");

    for (script, diagnostic) in [
        ("#!/bin/sh\nexit 1\n", "git could not list product sources"),
        (
            "#!/bin/sh\nprintf '\\377.rs\\0'\n",
            "non-UTF-8 product source path",
        ),
    ] {
        let directory = fixture("linux");
        write(
            directory.path().join("coverage.lcov"),
            coverage_report(directory.path(), 1),
        );
        let bin = directory.path().join("product-git-bin");
        fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        write(&git, script);
        let mut permissions = fs::metadata(&git).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&git, permissions).unwrap();
        let output = command()
            .args([
                "validate-coverage-report",
                "quality.toml",
                "app",
                "coverage.lcov",
                "--repository-root",
            ])
            .arg(directory.path())
            .env("PATH", &bin)
            .output()
            .unwrap();
        assert_failure(output, diagnostic);
    }

    let directory = fixture("linux");
    fs::remove_file(directory.path().join("app/src/lib.rs")).unwrap();
    write(directory.path().join("coverage.lcov"), "TN:\n");
    let output = run(
        directory.path(),
        &[
            "validate-coverage-report",
            "quality.toml",
            "app",
            "coverage.lcov",
        ],
    );
    assert_failure(output, "could not inspect product source");
}

fn add_git(bin: &Path) {
    let git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(git.status.success());
    let git = String::from_utf8(git.stdout).unwrap();
    std::os::unix::fs::symlink(git.trim(), bin.join("git")).unwrap();
}

fn second_target(name: &str, root: &str) -> String {
    format!(
        r#"
[[targets]]
name = "{name}"
kind = "rust"
platform = "linux"
cargo_manifest = "{root}/Cargo.toml"
product_sources = ["{root}/src"]
test_sources = ["{root}/tests"]
generated = []
third_party = []
supporting_files = []
"#
    )
}

fn add_second_target_files(root: &Path) {
    write(
        root.join("app2/Cargo.toml"),
        "[package]\nname = \"app2\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root.join("app2/src/lib.rs"), "pub fn second() {}\n");
    write(root.join("app2/tests/test.rs"), "#[test]\nfn test() {}\n");
    Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .unwrap();
}

#[test]
fn rejects_empty_duplicate_and_overlapping_target_registrations() {
    let directory = fixture("linux");
    let empty_manifest = manifest("linux")
        .split("[[targets]]")
        .next()
        .unwrap()
        .replace("version = 1", "version = 1\ntargets = []");
    write(directory.path().join("quality.toml"), empty_manifest);
    let output = run(
        directory.path(),
        &["validate-quality-manifest", "quality.toml"],
    );
    assert_failure(output, "at least one target");

    add_second_target_files(directory.path());
    write(
        directory.path().join("quality.toml"),
        format!("{}{}", manifest("linux"), second_target("app", "app2")),
    );
    let output = run(
        directory.path(),
        &["validate-quality-manifest", "quality.toml"],
    );
    assert_failure(output, "duplicate quality target");

    write(
        directory.path().join("quality.toml"),
        format!("{}{}", manifest("linux"), second_target("app2", "app")),
    );
    let output = run(
        directory.path(),
        &["validate-quality-manifest", "quality.toml"],
    );
    assert_failure(output, "overlaps");
}

#[test]
fn rejects_invalid_quality_command_arguments_and_paths() {
    let directory = fixture("linux");
    for (arguments, diagnostic) in [
        (
            vec!["validate-quality-manifest", "quality.toml", "extra"],
            "invalid quality command arguments",
        ),
        (
            vec![
                "validate-quality-manifest",
                "quality.toml",
                "--repository-root",
            ],
            "exactly once",
        ),
        (
            vec![
                "validate-quality-manifest",
                "quality.toml",
                "--repository-root",
                ".",
                "--repository-root",
                ".",
            ],
            "exactly once",
        ),
    ] {
        let output = command().args(arguments).output().unwrap();
        assert_failure(output, diagnostic);
    }

    let missing_root = directory.path().join("missing-root");
    let output = command()
        .args([
            "validate-quality-manifest",
            "quality.toml",
            "--repository-root",
        ])
        .arg(&missing_root)
        .output()
        .unwrap();
    assert_failure(output, "could not access repository root");

    let output = run(
        directory.path(),
        &["validate-quality-manifest", "missing.toml"],
    );
    assert_failure(output, "could not read quality manifest");

    let outside = TempDir::new();
    let link = directory.path().join("outside-link");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    write(
        directory.path().join("quality.toml"),
        manifest("linux").replace("app/src", "outside-link"),
    );
    let output = run(
        directory.path(),
        &["validate-quality-manifest", "quality.toml"],
    );
    assert_failure(output, "resolves outside the repository");
}

fn fake_tools(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    add_git(&bin);
    let rustc = bin.join("rustc");
    write(&rustc, "#!/bin/sh\necho 'rustc 1.98.1 (pinned)'\n");
    let cargo = bin.join("cargo");
    write(
        &cargo,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$QUALITY_COMMAND_LOG"
if [ "$QUALITY_FAIL" = "version-$1" ]; then exit 9; fi
if [ "$QUALITY_FAIL" = "signal-$1" ]; then kill -9 $$; fi
if [ "$QUALITY_FAIL" = "remove-cargo" ] && [ "$1 $2" = "llvm-cov --version" ]; then
  echo 'cargo-llvm-cov 0.9.1'
  /bin/rm "$0"
  exit 0
fi
case "$1 $2" in
  "--version ") echo 'cargo 1.98.1 (pinned)'; exit 0 ;;
  "llvm-cov --version") echo 'cargo-llvm-cov 0.9.1'; exit 0 ;;
esac
if [ "$QUALITY_FAIL" = "$1" ]; then exit 9; fi
if [ "$1" = "llvm-cov" ]; then
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--output-path" ]; then
      shift
      if [ "$QUALITY_MALFORMED" = 1 ]; then
        printf '%s\n' 'malformed' > "$1"
      else
        printf 'SF:%s/%s\nDA:1,1\nend_of_record\n' "$QUALITY_ROOT" "$QUALITY_PRODUCT_SOURCE" > "$1"
      fi
      break
    fi
    shift
  done
fi
exit 0
"#,
    );
    for path in [&rustc, &cargo] {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    bin
}

fn run_target(root: &Path, bin: &Path, fail: &str, malformed: bool) -> Output {
    command()
        .args(["run-quality-target", "quality.toml", "app"])
        .arg("--repository-root")
        .arg(root)
        .env("PATH", bin)
        .env("QUALITY_ROOT", root)
        .env("QUALITY_PRODUCT_SOURCE", "app/src/lib.rs")
        .env("QUALITY_COMMAND_LOG", root.join("commands.log"))
        .env("QUALITY_FAIL", fail)
        .env("QUALITY_MALFORMED", if malformed { "1" } else { "0" })
        .output()
        .unwrap()
}

#[test]
fn runs_every_rust_gate_with_pinned_tools_and_validates_reports() {
    let directory = fixture("linux");
    write(directory.path().join("app/vendor.file"), "excluded\n");
    write(
        directory.path().join("quality.toml"),
        manifest("linux").replace(
            "third_party = [\"app/vendor\"]",
            "third_party = [\"app/vendor\", \"app/vendor.file\"]",
        ),
    );
    let bin = fake_tools(directory.path());
    let output = run_target(directory.path(), &bin, "", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Quality target app passed.\n");
    let log = fs::read_to_string(directory.path().join("commands.log")).unwrap();
    for expected in [
        "fmt --manifest-path",
        "clippy --manifest-path",
        "llvm-cov --manifest-path",
    ] {
        assert!(log.contains(expected), "missing `{expected}` in {log}");
    }
    assert!(!log.contains("test --manifest-path"));
    assert!(!log.contains("mutants"));
    assert!(log.contains("--fail-under-lines 100"));
}

#[test]
fn runner_fails_closed_for_missing_tools_failed_gates_and_malformed_reports() {
    let directory = fixture("linux");
    let empty_path = directory.path().join("empty-bin");
    fs::create_dir(&empty_path).unwrap();
    add_git(&empty_path);
    let output = run_target(directory.path(), &empty_path, "", false);
    assert_failure(output, "rustc");

    let bin = fake_tools(directory.path());
    let output = run_target(directory.path(), &bin, "", true);
    assert_failure(output, "malformed coverage report");

    for (failure, diagnostic) in [
        ("fmt", "formatting failed"),
        ("signal-fmt", "status signal"),
        ("clippy", "static analysis failed"),
        ("llvm-cov", "tests and coverage failed"),
        ("version-llvm-cov", "could not report its version"),
        ("remove-cargo", "could not run formatting"),
    ] {
        let output = run_target(directory.path(), &bin, failure, false);
        assert_failure(output, diagnostic);
    }
}

#[test]
fn runner_rejects_wrong_versions_and_unavailable_cargo() {
    let directory = fixture("linux");
    let bin = fake_tools(directory.path());
    write(bin.join("rustc"), "#!/bin/sh\necho 'rustc 1.98.10'\n");
    let mut permissions = fs::metadata(bin.join("rustc")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin.join("rustc"), permissions).unwrap();
    let output = run_target(directory.path(), &bin, "", false);
    assert_failure(output, "expected `rustc 1.98.1`");

    write(bin.join("rustc"), "#!/bin/sh\nprintf '\\377'\n");
    let mut permissions = fs::metadata(bin.join("rustc")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bin.join("rustc"), permissions).unwrap();
    let output = run_target(directory.path(), &bin, "", false);
    assert_failure(output, "non-UTF-8 version");

    let no_cargo = directory.path().join("no-cargo");
    fs::create_dir(&no_cargo).unwrap();
    add_git(&no_cargo);
    fs::copy(bin.join("rustc"), no_cargo.join("rustc")).unwrap();
    write(no_cargo.join("rustc"), "#!/bin/sh\necho 'rustc 1.98.1'\n");
    let mut permissions = fs::metadata(no_cargo.join("rustc")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(no_cargo.join("rustc"), permissions).unwrap();
    let output = run_target(directory.path(), &no_cargo, "", false);
    assert_failure(output, "cargo llvm-cov");
}

#[test]
fn runner_reports_quality_output_directory_creation_failures() {
    let directory = fixture("linux");
    write(
        directory.path().join("app/target/quality"),
        "not a directory\n",
    );
    let bin = fake_tools(directory.path());
    let output = run_target(directory.path(), &bin, "", false);
    assert_failure(output, "could not create quality output directory");
}
