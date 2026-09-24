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
            "repo-policy-mutation-test-{}-{id}",
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

fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn quality_manifest() -> &'static str {
    r#"version = 1

[tools]
rust = "1.98.1"
cargo_llvm_cov = "0.9.1"
cargo_mutants = "27.1.0"

[[targets]]
name = "app"
kind = "rust"
platform = "linux"
cargo_manifest = "app/Cargo.toml"
product_sources = ["app/src"]
test_sources = ["app/tests"]
generated = ["app/generated"]
third_party = ["app/vendor"]
supporting_files = ["config/behavior.toml"]
"#
}

fn mutation_manifest() -> &'static str {
    r#"version = 1
behavior_paths = ["config/behavior.toml"]

[[groups]]
name = "parser"
target = "app"
product_file = "app/src/lib.rs"
function_regex = "parse_(body|field)"
test_paths = ["app/tests/parser.rs"]
behavior_paths = ["config/behavior.toml"]
test_target = "parser"

[[groups.exceptions]]
outcome = "unviable"
pattern = "replace parse_body.*Default::default"
reason = "The generated replacement cannot satisfy the return type."
"#
}

fn mutants_json() -> &'static str {
    r#"[
  {
    "name": "src/lib.rs:10: replace parse_body with ()",
    "file": "src/lib.rs",
    "function": {"function_name": "parse_body"}
  }
]"#
}

fn outcomes_json(summary: &str, mutant_name: &str) -> String {
    let (reported_summary, counts) = match summary {
        "Caught" => ("CaughtMutant", (1, 0, 0, 0)),
        "Missed" => ("MissedMutant", (0, 1, 0, 0)),
        "Timeout" => ("Timeout", (0, 0, 1, 0)),
        "Unviable" => ("Unviable", (0, 0, 0, 1)),
        _ => panic!("unsupported test summary"),
    };
    format!(
        r#"{{
  "outcomes": [
    {{"scenario": "Baseline", "summary": "Success"}},
    {{"scenario": {{"Mutant": {{"name": {mutant_name:?}}}}}, "summary": "{reported_summary}"}}
  ],
  "total_mutants": 1,
  "missed": {},
  "caught": {},
  "timeout": {},
  "unviable": {},
  "success": 0,
  "cargo_mutants_version": "27.1.0"
}}"#,
        counts.1, counts.0, counts.2, counts.3
    )
}

fn fixture() -> TempDir {
    let directory = TempDir::new();
    for (path, contents) in [
        ("quality.toml", quality_manifest()),
        ("mutation.toml", mutation_manifest()),
        (
            "app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        ),
        ("app/src/lib.rs", "pub fn parse_body() {}\n"),
        ("app/tests/parser.rs", "#[test]\nfn parser() {}\n"),
        ("app/generated/bindings.rs", "generated!();\n"),
        ("app/vendor/dependency.rs", "third_party!();\n"),
        ("config/behavior.toml", "strict = true\n"),
        ("mutants.json", mutants_json()),
    ] {
        write(directory.path().join(path), contents);
    }
    directory
}

fn plan(directory: &TempDir, changed: &[u8]) -> Output {
    write(directory.path().join("changed-paths"), changed);
    command()
        .args([
            "mutation-plan",
            "quality.toml",
            "mutation.toml",
            "changed-paths",
            "mutants.json",
            "--repository-root",
        ])
        .arg(directory.path())
        .output()
        .unwrap()
}

fn validate_results(directory: &TempDir, outcomes: &str) -> Output {
    write(directory.path().join("outcomes.json"), outcomes);
    command()
        .args([
            "validate-mutation-results",
            "quality.toml",
            "mutation.toml",
            "parser",
            "outcomes.json",
            "--repository-root",
        ])
        .arg(directory.path())
        .output()
        .unwrap()
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
fn plans_product_test_configuration_mixed_and_empty_changes() {
    let expected_diff = r#"{"include":[{"name":"parser","cargo_manifest":"app/Cargo.toml","mutation_file":"src/lib.rs","function_regex":"parse_(body|field)","test_target":"parser","use_diff":true}]}"#;
    let expected_full = expected_diff.replace("true", "false");

    for (changed, mutants, expected) in [
        (&b"app/src/lib.rs\0"[..], mutants_json(), expected_diff),
        (&b"app/tests/parser.rs\0"[..], "[]", expected_full.as_str()),
        (&b"config/behavior.toml\0"[..], "[]", expected_full.as_str()),
        (
            &b"app/src/lib.rs\0app/tests/parser.rs\0"[..],
            mutants_json(),
            expected_full.as_str(),
        ),
        (&b"README.md\0"[..], "[]", r#"{"include":[]}"#),
        (&b"\0"[..], "[]", r#"{"include":[]}"#),
    ] {
        let directory = fixture();
        write(directory.path().join("mutants.json"), mutants);
        let output = plan(&directory, changed);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }

    for name in ["2", "-", "_"] {
        let directory = fixture();
        write(
            directory.path().join("mutation.toml"),
            mutation_manifest().replace("name = \"parser\"", &format!("name = {name:?}")),
        );
        write(directory.path().join("mutants.json"), "[]");
        let output = plan(&directory, b"README.md\0");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn rejects_unmapped_product_mutants_tests_and_behavior_configuration() {
    let directory = fixture();
    write(
        directory.path().join("mutants.json"),
        mutants_json().replace("parse_body", "unmapped_function"),
    );
    assert_failure(plan(&directory, b"app/src/lib.rs\0"), "unmapped mutant");

    let directory = fixture();
    write(
        directory.path().join("app/tests/unmapped.rs"),
        "#[test]\nfn test() {}\n",
    );
    assert_failure(
        plan(&directory, b"app/tests/unmapped.rs\0"),
        "unmapped test path",
    );

    let directory = fixture();
    write(
        directory.path().join("mutation.toml"),
        mutation_manifest().replacen(
            "behavior_paths = [\"config/behavior.toml\"]",
            "behavior_paths = [\"config/behavior.toml\", \"config/unmapped.toml\"]",
            1,
        ),
    );
    write(
        directory.path().join("config/unmapped.toml"),
        "enabled = true\n",
    );
    assert_failure(
        plan(&directory, b"config/unmapped.toml\0"),
        "unmapped behavior path",
    );
}

#[test]
fn rejects_malformed_ambiguous_or_unsafe_mutation_inputs() {
    for (mutation, changed, mutants, diagnostic) in [
        ("not toml", &b"README.md\0"[..], mutants_json(), "TOML"),
        (
            &mutation_manifest().replace("version = 1", "version = 2"),
            &b"README.md\0"[..],
            mutants_json(),
            "version",
        ),
        (
            &mutation_manifest().replace("parse_(body|field)", "("),
            &b"README.md\0"[..],
            mutants_json(),
            "regular expression",
        ),
        (
            &mutation_manifest().replace("name = \"parser\"", "name = \"Parser\""),
            &b"README.md\0"[..],
            mutants_json(),
            "group name",
        ),
        (
            &mutation_manifest().replace("app/src/lib.rs", "app/tests/parser.rs"),
            &b"README.md\0"[..],
            mutants_json(),
            "product source",
        ),
        (
            &mutation_manifest().replace("test_target = \"parser\"", "test_target = \"bad value\""),
            &b"README.md\0"[..],
            mutants_json(),
            "test target",
        ),
        (
            mutation_manifest(),
            &b"app/src/lib.rs"[..],
            mutants_json(),
            "NUL-terminated",
        ),
        (
            mutation_manifest(),
            &b"app/src/lib.rs\0"[..],
            "not json",
            "JSON",
        ),
        (
            "version = 1\nbehavior_paths = []\ngroups = []\n",
            &b"README.md\0"[..],
            "[]",
            "at least one group",
        ),
        (
            &mutation_manifest().replace("outcome = \"unviable\"", "outcome = \"other\""),
            &b"README.md\0"[..],
            "[]",
            "outcome",
        ),
        (
            &mutation_manifest().replace(
                "pattern = \"replace parse_body.*Default::default\"",
                "pattern = \"(\"",
            ),
            &b"README.md\0"[..],
            "[]",
            "exception regular expression",
        ),
        (
            &mutation_manifest().replace(
                "reason = \"The generated replacement cannot satisfy the return type.\"",
                "reason = \"\"",
            ),
            &b"README.md\0"[..],
            "[]",
            "reason",
        ),
        (
            &mutation_manifest().replacen(
                "behavior_paths = [\"config/behavior.toml\"]",
                "behavior_paths = []",
                1,
            ),
            &b"README.md\0"[..],
            "[]",
            "undeclared behavior path",
        ),
        (
            &mutation_manifest().replace(
                "test_paths = [\"app/tests/parser.rs\"]",
                "test_paths = [\"app/tests/parser.rs\", \"app/tests/parser.rs\"]",
            ),
            &b"README.md\0"[..],
            "[]",
            "unique and sorted",
        ),
        (
            &mutation_manifest().replace(
                "test_paths = [\"app/tests/parser.rs\"]",
                "test_paths = [\"config/behavior.toml\"]",
            ),
            &b"README.md\0"[..],
            "[]",
            "outside its test sources",
        ),
        (
            &mutation_manifest().replace(
                "behavior_paths = [\"config/behavior.toml\"]\ntest_target",
                "behavior_paths = [\"config/behavior.toml\", \"config/behavior.toml\"]\ntest_target",
            ),
            &b"README.md\0"[..],
            "[]",
            "unique and sorted",
        ),
        (
            &mutation_manifest().replace("product_file = \"app/src/lib.rs\"", "product_file = \"app/src\""),
            &b"README.md\0"[..],
            "[]",
            "regular file",
        ),
    ] {
        let directory = fixture();
        write(directory.path().join("mutation.toml"), mutation);
        write(directory.path().join("mutants.json"), mutants);
        assert_failure(plan(&directory, changed), diagnostic);
    }

    let directory = fixture();
    let duplicate_group = mutation_manifest()
        .split_once("[[groups]]")
        .map(|(_, group)| format!("{}\n[[groups]]{}", mutation_manifest(), group))
        .unwrap();
    write(directory.path().join("mutation.toml"), duplicate_group);
    assert_failure(plan(&directory, b"README.md\0"), "group names");

    let directory = fixture();
    write(
        directory.path().join("mutation.toml"),
        mutation_manifest().replacen(
            "behavior_paths = [\"config/behavior.toml\"]",
            "behavior_paths = [\"config/missing.toml\"]",
            1,
        ),
    );
    assert_failure(
        plan(&directory, b"README.md\0"),
        "could not inspect mutation behavior_paths",
    );

    let directory = fixture();
    write(directory.path().join("changed-paths"), [0xff, 0]);
    assert_failure(plan(&directory, &[0xff, 0]), "valid UTF-8");

    let directory = fixture();
    assert_failure(plan(&directory, b"README.md\0README.md\0"), "duplicate");

    let directory = fixture();
    write(
        directory.path().join("mutants.json"),
        r#"[{"name":"unknown","file":"src/lib.rs","function":null}]"#,
    );
    assert_failure(plan(&directory, b"app/src/lib.rs\0"), "unmapped mutant");
}

#[test]
fn rejects_missing_inputs_unknown_groups_and_products_outside_the_package() {
    let directory = fixture();
    write(directory.path().join("quality.toml"), "not toml");
    assert_failure(plan(&directory, b"README.md\0"), "quality manifest TOML");

    let directory = fixture();
    fs::remove_file(directory.path().join("mutation.toml")).unwrap();
    assert_failure(
        plan(&directory, b"README.md\0"),
        "could not read mutation manifest",
    );

    let directory = fixture();
    fs::remove_file(directory.path().join("mutants.json")).unwrap();
    assert_failure(
        plan(&directory, b"README.md\0"),
        "could not read cargo-mutants list",
    );

    let directory = fixture();
    let output = command()
        .args([
            "mutation-plan",
            "quality.toml",
            "mutation.toml",
            "missing-paths",
            "mutants.json",
            "--repository-root",
        ])
        .arg(directory.path())
        .output()
        .unwrap();
    assert_failure(output, "could not read changed paths");

    let directory = fixture();
    write(
        directory.path().join("outcomes.json"),
        outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()"),
    );
    let output = command()
        .args([
            "validate-mutation-results",
            "quality.toml",
            "mutation.toml",
            "unknown",
            "outcomes.json",
            "--repository-root",
        ])
        .arg(directory.path())
        .output()
        .unwrap();
    assert_failure(output, "unknown mutation group");

    let directory = fixture();
    write(directory.path().join("quality.toml"), "not toml");
    assert_failure(
        validate_results(
            &directory,
            &outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()"),
        ),
        "quality manifest TOML",
    );

    let directory = fixture();
    write(directory.path().join("mutation.toml"), "not toml");
    assert_failure(
        validate_results(
            &directory,
            &outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()"),
        ),
        "mutation manifest TOML",
    );

    let directory = fixture();
    assert_failure(
        validate_results(&directory, "not json"),
        "mutation outcomes JSON",
    );

    let directory = fixture();
    let output = command()
        .args([
            "validate-mutation-results",
            "quality.toml",
            "mutation.toml",
            "parser",
            "missing.json",
            "--repository-root",
        ])
        .arg(directory.path())
        .output()
        .unwrap();
    assert_failure(output, "could not read mutation outcomes");

    let directory = fixture();
    write(
        directory.path().join("other/Cargo.toml"),
        "[package]\nname = \"other\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        directory.path().join("quality.toml"),
        quality_manifest().replace("app/Cargo.toml", "other/Cargo.toml"),
    );
    assert_failure(
        plan(&directory, b"app/src/lib.rs\0"),
        "outside its Cargo package",
    );
}

#[test]
fn validates_killed_mutants_and_approved_unviable_exceptions() {
    let directory = fixture();
    let output = validate_results(
        &directory,
        &outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()"),
    );
    assert!(output.status.success());

    let output = validate_results(
        &directory,
        &outcomes_json(
            "Unviable",
            "src/lib.rs:10: replace parse_body with Default::default",
        ),
    );
    assert!(output.status.success());

    write(
        directory.path().join("mutation.toml"),
        mutation_manifest()
            .replace("outcome = \"unviable\"", "outcome = \"missed\"")
            .replace("Default::default", "equivalent replacement"),
    );
    let output = validate_results(
        &directory,
        &outcomes_json(
            "Missed",
            "src/lib.rs:10: replace parse_body with equivalent replacement",
        ),
    );
    assert!(output.status.success());
}

#[test]
fn rejects_missed_timed_out_unapproved_and_wrong_version_results() {
    for (manifest, outcomes, diagnostic) in [
        (
            mutation_manifest().to_owned(),
            outcomes_json("Missed", "src/lib.rs:10: replace parse_body with ()"),
            "missed",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Timeout", "src/lib.rs:10: replace parse_body with ()"),
            "timed out",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Unviable", "src/lib.rs:10: unsupported mutation"),
            "unapproved unviable",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()")
                .replace("27.1.0", "27.0.0"),
            "version",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()")
                .replace("\"total_mutants\": 1", "\"total_mutants\": 2"),
            "inconsistent",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()").replace(
                "    {\"scenario\": \"Baseline\", \"summary\": \"Success\"},\n",
                "",
            ),
            "successful baseline",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()")
                .replace("\"summary\": \"Success\"", "\"summary\": \"Failure\""),
            "baseline failed",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()")
                .replace("\"summary\": \"CaughtMutant\"", "\"summary\": \"Failure\""),
            "unknown mutation outcome",
        ),
        (
            mutation_manifest().to_owned(),
            r#"{
  "outcomes": [{"scenario": "Baseline", "summary": "Success"}],
  "total_mutants": 0,
  "missed": 0,
  "caught": 0,
  "timeout": 0,
  "unviable": 0,
  "success": 0,
  "cargo_mutants_version": "27.1.0"
}"#
            .to_owned(),
            "no mutants",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json("Caught", "src/lib.rs:10: replace parse_body with ()")
                .replace("\"caught\": 1", "\"caught\": 0")
                .replace("\"success\": 0", "\"success\": 1"),
            "inconsistent",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json(
                "Unviable",
                "src/lib.rs:10: replace parse_body with Default::default",
            )
            .replace("{\"Mutant\": {\"name\":", "{\"Other\": {\"name\":"),
            "malformed mutant scenario",
        ),
        (
            mutation_manifest().to_owned(),
            outcomes_json(
                "Unviable",
                "src/lib.rs:10: replace parse_body with Default::default",
            )
            .replace("{\"Mutant\": {\"name\":", "{\"Mutant\": {\"other\":"),
            "malformed mutant scenario",
        ),
    ] {
        let directory = fixture();
        write(directory.path().join("mutation.toml"), manifest);
        assert_failure(validate_results(&directory, &outcomes), diagnostic);
    }
}

#[test]
fn runs_bounded_affected_group_mutation_testing_without_secrets() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow =
        fs::read_to_string(manifest.join("../../.github/workflows/mutation-testing.yml")).unwrap();

    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("permissions:\n  contents: read"));
    assert!(workflow.contains("runs-on: ubuntu-24.04"));
    assert!(workflow.contains("timeout-minutes: 5"));
    assert!(workflow.contains("rust@1.98.1"));
    assert!(workflow.contains("cargo-mutants@27.1.0"));
    assert!(workflow.contains("mutation-plan"));
    assert!(workflow.contains("fromJSON(needs.plan.outputs.matrix)"));
    assert!(workflow.contains("timeout --signal=TERM --kill-after=5s 240s"));
    assert!(workflow.contains("mutation_status=$?"));
    assert!(workflow.contains("if [ \"$mutation_status\" -eq 124 ]"));
    assert!(!workflow.contains(") || true"));
    assert!(workflow.contains("--no-shuffle"));
    assert!(workflow.contains("function_pattern=${FUNCTION_REGEX#^}"));
    assert!(workflow.contains("function_pattern=${function_pattern%\\$}"));
    assert!(workflow.contains("mutant_regex=\"( in |replace )${function_pattern}( ->| with|$)\""));
    assert!(workflow.contains("--re \"$mutant_regex\""));
    assert!(!workflow.contains("--re \"$FUNCTION_REGEX\""));
    assert!(workflow.contains("--in-diff"));
    assert!(workflow.contains("validate-mutation-results"));
    assert!(workflow.contains("actions/upload-artifact@"));
    assert!(workflow.contains("if: always()"));
    assert!(workflow.contains("name: Mutation testing"));
    assert!(workflow.contains("PLAN_RESULT: ${{ needs.plan.result }}"));
    assert!(workflow.contains("MUTATION_RESULT: ${{ needs.mutate.result }}"));
    assert!(!workflow.contains("pull_request_target"));
    assert!(!workflow.contains("secrets:"));
    assert!(!workflow.contains(".githooks"));
}
