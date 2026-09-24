use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const SUPPORTED_VERSION: u64 = 1;
const RUST_RUNNER: &str = "ubuntu-24.04";
const MACOS_RUNNER: &str = "macos-15";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityManifest {
    version: u64,
    tools: Tools,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    rust: String,
    cargo_llvm_cov: String,
    cargo_mutants: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    name: String,
    kind: String,
    platform: String,
    cargo_manifest: String,
    product_sources: Vec<String>,
    test_sources: Vec<String>,
    generated: Vec<String>,
    third_party: Vec<String>,
    supporting_files: Vec<String>,
}

struct Invocation {
    repository_root: PathBuf,
    positional: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationManifest {
    version: u64,
    behavior_paths: Vec<String>,
    groups: Vec<MutationGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationGroup {
    name: String,
    target: String,
    product_file: String,
    function_regex: String,
    test_paths: Vec<String>,
    behavior_paths: Vec<String>,
    test_target: String,
    #[serde(default)]
    exceptions: Vec<MutationException>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationException {
    outcome: String,
    pattern: String,
    reason: String,
}

#[derive(Deserialize)]
struct ListedMutant {
    name: String,
    file: String,
    function: Option<ListedFunction>,
}

#[derive(Deserialize)]
struct ListedFunction {
    function_name: String,
}

#[derive(Default, Serialize)]
struct MutationMatrix {
    include: Vec<MutationMatrixEntry>,
}

#[derive(Serialize)]
struct MutationMatrixEntry {
    name: String,
    cargo_manifest: String,
    mutation_file: String,
    function_regex: String,
    test_target: String,
    use_diff: bool,
}

#[derive(Deserialize)]
struct MutationOutcomes {
    outcomes: Vec<MutationOutcome>,
    total_mutants: usize,
    missed: usize,
    caught: usize,
    timeout: usize,
    unviable: usize,
    success: usize,
    cargo_mutants_version: String,
}

#[derive(Deserialize)]
struct MutationOutcome {
    scenario: serde_json::Value,
    summary: String,
}

pub fn validate_manifest(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 1)?;
    load_and_validate(&invocation.repository_root, &invocation.positional[0], true)?;
    println!("Quality manifest is valid.");
    Ok(())
}

pub fn print_matrix(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 1)?;
    let manifest = load_and_validate(&invocation.repository_root, &invocation.positional[0], true)?;
    let entries = manifest
        .targets
        .iter()
        .map(|target| {
            let runner = if target.platform == "linux" {
                RUST_RUNNER
            } else {
                MACOS_RUNNER
            };
            format!("{{\"name\":\"{}\",\"runner\":\"{runner}\"}}", target.name)
        })
        .collect::<Vec<_>>()
        .join(",");
    println!("{{\"include\":[{entries}]}}");
    Ok(())
}

pub fn validate_coverage(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 3)?;
    let manifest = load_and_validate(
        &invocation.repository_root,
        &invocation.positional[0],
        false,
    )?;
    let target = find_target(&manifest, &invocation.positional[1])?;
    let report = repository_path(&invocation.repository_root, &invocation.positional[2])?;
    validate_lcov(&invocation.repository_root, target, &report)?;
    println!("Coverage for {} is 100%.", target.name);
    Ok(())
}

pub fn mutation_plan(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 4)?;
    let quality = load_and_validate(
        &invocation.repository_root,
        &invocation.positional[0],
        false,
    )?;
    let mutation = load_mutation_manifest(
        &invocation.repository_root,
        &invocation.positional[1],
        &quality,
    )?;
    let changed = read_changed_paths(&invocation.repository_root, &invocation.positional[2])?;
    let mutants_path = repository_path(&invocation.repository_root, &invocation.positional[3])?;
    let mutants_json = match fs::read_to_string(&mutants_path) {
        Ok(contents) => contents,
        Err(error) => {
            return Err(format!(
                "could not read cargo-mutants list `{}`: {error}",
                mutants_path.display()
            ));
        }
    };
    let mutants: Vec<ListedMutant> = match serde_json::from_str(&mutants_json) {
        Ok(mutants) => mutants,
        Err(error) => {
            return Err(format!("cargo-mutants list JSON is malformed: {error}"));
        }
    };
    let matrix = plan_mutations(&quality, &mutation, &changed, &mutants)?;
    let json = serde_json::to_string(&matrix)
        .expect("mutation matrix contains only JSON-serializable strings and booleans");
    println!("{json}");
    Ok(())
}

pub fn validate_mutation_results(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 4)?;
    let quality = load_and_validate(
        &invocation.repository_root,
        &invocation.positional[0],
        false,
    )?;
    let mutation = load_mutation_manifest(
        &invocation.repository_root,
        &invocation.positional[1],
        &quality,
    )?;
    let group = match mutation
        .groups
        .iter()
        .find(|group| group.name == invocation.positional[2])
    {
        Some(group) => group,
        None => {
            return Err(format!(
                "unknown mutation group `{}`",
                invocation.positional[2]
            ));
        }
    };
    let outcomes_path = repository_path(&invocation.repository_root, &invocation.positional[3])?;
    let outcomes_json = match fs::read_to_string(&outcomes_path) {
        Ok(contents) => contents,
        Err(error) => {
            return Err(format!(
                "could not read mutation outcomes `{}`: {error}",
                outcomes_path.display()
            ));
        }
    };
    let outcomes: MutationOutcomes = match serde_json::from_str(&outcomes_json) {
        Ok(outcomes) => outcomes,
        Err(error) => return Err(format!("mutation outcomes JSON is malformed: {error}")),
    };
    validate_outcomes(group, &quality.tools.cargo_mutants, &outcomes)?;
    println!("Mutation group {} passed.", group.name);
    Ok(())
}

pub fn run_target(arguments: &[String]) -> Result<(), String> {
    let invocation = parse_invocation(arguments, 2)?;
    let manifest = load_and_validate(&invocation.repository_root, &invocation.positional[0], true)?;
    let target = find_target(&manifest, &invocation.positional[1])?;
    verify_tools(&invocation.repository_root, &manifest.tools)?;

    let cargo_manifest = repository_path(&invocation.repository_root, &target.cargo_manifest)?;
    let cargo_manifest = cargo_manifest.to_string_lossy().into_owned();
    run_command(
        &invocation.repository_root,
        "formatting",
        "cargo",
        &["fmt", "--manifest-path", &cargo_manifest, "--", "--check"],
    )?;
    run_command(
        &invocation.repository_root,
        "static analysis",
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            &cargo_manifest,
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    let output_directory =
        cargo_manifest_parent(&invocation.repository_root, target)?.join("target/quality");
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "could not create quality output directory `{}`: {error}",
            output_directory.display()
        )
    })?;
    let coverage_report = output_directory.join(format!("{}-coverage.lcov", target.name));
    let coverage_report_text = coverage_report.to_string_lossy().into_owned();
    let exclusion_regex = coverage_exclusion_regex(target);
    run_command(
        &invocation.repository_root,
        "tests and coverage",
        "cargo",
        &[
            "llvm-cov",
            "--manifest-path",
            &cargo_manifest,
            "--lcov",
            "--output-path",
            &coverage_report_text,
            "--fail-under-lines",
            "100",
            "--ignore-filename-regex",
            &exclusion_regex,
        ],
    )?;
    validate_lcov(&invocation.repository_root, target, &coverage_report)?;

    println!("Quality target {} passed.", target.name);
    Ok(())
}

fn parse_invocation(arguments: &[String], positional_count: usize) -> Result<Invocation, String> {
    let mut repository_root = None;
    let mut positional = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--repository-root" {
            let Some(path) = arguments.next() else {
                return Err(
                    "--repository-root must be supplied exactly once with a path".to_owned(),
                );
            };
            if repository_root.replace(PathBuf::from(path)).is_some() {
                return Err(
                    "--repository-root must be supplied exactly once with a path".to_owned(),
                );
            }
        } else {
            positional.push(argument.clone());
        }
    }
    if positional.len() != positional_count {
        return Err("invalid quality command arguments; run with --help for usage".to_owned());
    }
    let repository_root = match repository_root {
        Some(path) => path,
        None => PathBuf::from("."),
    };
    let repository_root = fs::canonicalize(&repository_root).map_err(|error| {
        format!(
            "could not access repository root `{}`: {error}",
            repository_root.display()
        )
    })?;
    Ok(Invocation {
        repository_root,
        positional,
    })
}

fn load_and_validate(
    repository_root: &Path,
    manifest_path: &str,
    check_ownership: bool,
) -> Result<QualityManifest, String> {
    let path = repository_path(repository_root, manifest_path)?;
    let contents = fs::read_to_string(&path).map_err(|error| {
        format!(
            "could not read quality manifest `{}`: {error}",
            path.display()
        )
    })?;
    let manifest: QualityManifest = toml::from_str(&contents)
        .map_err(|error| format!("quality manifest TOML is malformed: {error}"))?;
    validate_structure(repository_root, &manifest)?;
    if check_ownership {
        validate_ownership(repository_root, &manifest)?;
    }
    Ok(manifest)
}

fn validate_structure(repository_root: &Path, manifest: &QualityManifest) -> Result<(), String> {
    if manifest.version != SUPPORTED_VERSION {
        return Err(format!(
            "unsupported quality manifest version {}; expected {SUPPORTED_VERSION}",
            manifest.version
        ));
    }
    for (name, version) in [
        ("rust", manifest.tools.rust.as_str()),
        ("cargo_llvm_cov", manifest.tools.cargo_llvm_cov.as_str()),
        ("cargo_mutants", manifest.tools.cargo_mutants.as_str()),
    ] {
        if !valid_version(version) {
            return Err(format!("tool `{name}` must have an exact numeric version"));
        }
    }
    if manifest.targets.is_empty() {
        return Err("quality manifest must register at least one target".to_owned());
    }

    let mut names = BTreeSet::new();
    let mut ownership = Vec::<(String, String)>::new();
    for target in &manifest.targets {
        if target.name.is_empty()
            || !target
                .name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(format!("invalid quality target name `{}`", target.name));
        }
        if !names.insert(&target.name) {
            return Err(format!("duplicate quality target `{}`", target.name));
        }
        if target.kind != "rust" {
            return Err(format!(
                "unsupported kind `{}` for target `{}`; only rust is permitted",
                target.kind, target.name
            ));
        }
        if !matches!(target.platform.as_str(), "linux" | "macos") {
            return Err(format!(
                "unsupported platform `{}` for target `{}`",
                target.platform, target.name
            ));
        }
        if target.product_sources.is_empty() {
            return Err(format!(
                "target `{}` must declare product_sources",
                target.name
            ));
        }
        if target.test_sources.is_empty() {
            return Err(format!(
                "target `{}` must declare test_sources",
                target.name
            ));
        }
        let categories = [
            ("cargo_manifest", vec![target.cargo_manifest.as_str()]),
            (
                "product_sources",
                target.product_sources.iter().map(String::as_str).collect(),
            ),
            (
                "test_sources",
                target.test_sources.iter().map(String::as_str).collect(),
            ),
            (
                "generated",
                target.generated.iter().map(String::as_str).collect(),
            ),
            (
                "third_party",
                target.third_party.iter().map(String::as_str).collect(),
            ),
            (
                "supporting_files",
                target.supporting_files.iter().map(String::as_str).collect(),
            ),
        ];
        let mut target_paths = Vec::new();
        for (category, paths) in categories {
            let mut previous = None;
            for path in paths {
                validate_relative_path(path)?;
                if previous.is_some_and(|previous| previous >= path) {
                    return Err(format!(
                        "target `{}` {category} paths must be unique and sorted",
                        target.name
                    ));
                }
                previous = Some(path);
                let absolute = repository_path(repository_root, path)?;
                if !absolute.exists() {
                    return Err(format!(
                        "target `{}` {category} path `{path}` does not exist",
                        target.name
                    ));
                }
                target_paths.push((category, path));
            }
        }
        for left in 0..target_paths.len() {
            for right in left + 1..target_paths.len() {
                if paths_overlap(target_paths[left].1, target_paths[right].1) {
                    return Err(format!(
                        "target `{}` has overlap between {} `{}` and {} `{}`",
                        target.name,
                        target_paths[left].0,
                        target_paths[left].1,
                        target_paths[right].0,
                        target_paths[right].1
                    ));
                }
            }
        }
        for (_, path) in target_paths {
            if let Some((owned_path, owner)) = ownership
                .iter()
                .find(|(owned_path, _)| paths_overlap(path, owned_path))
            {
                return Err(format!(
                    "path `{path}` overlaps `{owned_path}` owned by target `{owner}`"
                ));
            }
            ownership.push((path.to_owned(), target.name.clone()));
        }
    }
    Ok(())
}

fn validate_ownership(repository_root: &Path, manifest: &QualityManifest) -> Result<(), String> {
    let output = match Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(repository_root)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return Err(format!(
                "could not run git to inspect source ownership: {error}"
            ));
        }
    };
    if !output.status.success() {
        return Err(format!(
            "git could not inspect source ownership: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let records = output.stdout.split(|byte| *byte == 0);
    for record in records.filter(|record| !record.is_empty()) {
        let record = std::str::from_utf8(record)
            .map_err(|_| "git returned a non-UTF-8 tracked path".to_owned())?;
        let (metadata, path) = record
            .split_once('\t')
            .ok_or_else(|| "git returned malformed tracked-file metadata".to_owned())?;
        let mode = metadata
            .split_whitespace()
            .next()
            .ok_or_else(|| "git returned malformed tracked-file mode".to_owned())?;
        let is_source = path.ends_with(".rs") || mode == "100755";
        if is_source && owner(manifest, path).is_none() {
            return Err(format!(
                "repository-authored source `{path}` is not owned by a quality target"
            ));
        }
    }
    Ok(())
}

fn owner<'a>(manifest: &'a QualityManifest, path: &str) -> Option<&'a str> {
    manifest.targets.iter().find_map(|target| {
        let owned = std::iter::once(target.cargo_manifest.as_str())
            .chain(target.product_sources.iter().map(String::as_str))
            .chain(target.test_sources.iter().map(String::as_str))
            .chain(target.generated.iter().map(String::as_str))
            .chain(target.third_party.iter().map(String::as_str))
            .chain(target.supporting_files.iter().map(String::as_str))
            .any(|owned| path_is_within(path, owned));
        owned.then_some(target.name.as_str())
    })
}

fn validate_lcov(repository_root: &Path, target: &Target, report: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(report).map_err(|error| {
        format!(
            "could not read coverage report `{}`: {error}",
            report.display()
        )
    })?;
    let product_files = rust_files(repository_root, &target.product_sources)?;
    let mut reported = BTreeSet::new();
    let mut current_source: Option<PathBuf> = None;
    let mut current_has_line = false;

    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        if let Some(source) = line.strip_prefix("SF:") {
            if current_source.is_some() || source.is_empty() {
                return Err(format!(
                    "malformed coverage report at line {line_number}: invalid source record"
                ));
            }
            let source = PathBuf::from(source);
            let source = if source.is_absolute() {
                source
            } else {
                repository_root.join(source)
            };
            let source = fs::canonicalize(&source).map_err(|_| {
                format!("malformed coverage report at line {line_number}: source does not exist")
            })?;
            if !product_files.contains(&source) {
                return Err(format!(
                    "coverage source `{}` is outside product sources for `{}`",
                    source.display(),
                    target.name
                ));
            }
            if !reported.insert(source.clone()) {
                return Err(format!(
                    "malformed coverage report: duplicate source `{}`",
                    source.display()
                ));
            }
            current_source = Some(source);
            current_has_line = false;
        } else if let Some(data) = line.strip_prefix("DA:") {
            let source = current_source.as_ref().ok_or_else(|| {
                format!("malformed coverage report at line {line_number}: line without source")
            })?;
            let (line, count) = data.split_once(',').ok_or_else(|| {
                format!("malformed coverage report at line {line_number}: invalid line data")
            })?;
            let line = line.parse::<u64>().map_err(|_| {
                format!("malformed coverage report at line {line_number}: invalid line number")
            })?;
            let count = count.parse::<u64>().map_err(|_| {
                format!("malformed coverage report at line {line_number}: invalid execution count")
            })?;
            if line == 0 {
                return Err(format!(
                    "malformed coverage report at line {line_number}: line number must be positive"
                ));
            }
            if count == 0 {
                return Err(format!("uncovered line {line} in `{}`", source.display()));
            }
            current_has_line = true;
        } else if line == "end_of_record" {
            let source = current_source.take().ok_or_else(|| {
                format!("malformed coverage report at line {line_number}: record has no source")
            })?;
            if !current_has_line {
                return Err(format!(
                    "malformed coverage report: source `{}` has no line data",
                    source.display()
                ));
            }
        } else if !(line.is_empty()
            || line.starts_with("TN:")
            || line.starts_with("FN:")
            || line.starts_with("FNDA:")
            || line.starts_with("FNF:")
            || line.starts_with("FNH:")
            || line.starts_with("LF:")
            || line.starts_with("LH:")
            || line.starts_with("BRDA:")
            || line.starts_with("BRF:")
            || line.starts_with("BRH:"))
        {
            return Err(format!(
                "malformed coverage report at line {line_number}: unknown record"
            ));
        }
    }
    if current_source.is_some() {
        return Err("malformed coverage report: unterminated source record".to_owned());
    }
    if let Some(missing) = product_files.difference(&reported).next() {
        return Err(format!(
            "coverage report is missing product source `{}`",
            missing.display()
        ));
    }
    Ok(())
}

fn rust_files(repository_root: &Path, roots: &[String]) -> Result<BTreeSet<PathBuf>, String> {
    let output = match Command::new("git")
        .args(["ls-files", "-z", "--"])
        .args(roots)
        .current_dir(repository_root)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return Err(format!(
                "could not run git to list product sources: {error}"
            ));
        }
    };
    if !output.status.success() {
        return Err(format!(
            "git could not list product sources: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut files = BTreeSet::new();
    for path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)
            .map_err(|_| "git returned a non-UTF-8 product source path".to_owned())?;
        if !path.ends_with(".rs") {
            continue;
        }
        let path = repository_path(repository_root, path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "could not inspect product source `{}`: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "product source `{}` cannot be a symbolic link",
                path.display()
            ));
        }
        files.insert(path);
    }
    if files.is_empty() {
        return Err("product_sources contain no Rust source files".to_owned());
    }
    Ok(files)
}

fn load_mutation_manifest(
    repository_root: &Path,
    path: &str,
    quality: &QualityManifest,
) -> Result<MutationManifest, String> {
    let path = repository_path(repository_root, path)?;
    let contents = fs::read_to_string(&path).map_err(|error| {
        format!(
            "could not read mutation manifest `{}`: {error}",
            path.display()
        )
    })?;
    let manifest: MutationManifest = toml::from_str(&contents)
        .map_err(|error| format!("mutation manifest TOML is malformed: {error}"))?;
    validate_mutation_manifest(repository_root, quality, &manifest)?;
    Ok(manifest)
}

fn validate_mutation_manifest(
    repository_root: &Path,
    quality: &QualityManifest,
    manifest: &MutationManifest,
) -> Result<(), String> {
    if manifest.version != 1 {
        return Err(format!(
            "unsupported mutation manifest version {}; expected 1",
            manifest.version
        ));
    }
    validate_sorted_paths(
        repository_root,
        "mutation behavior_paths",
        &manifest.behavior_paths,
    )?;
    if manifest.groups.is_empty() {
        return Err("mutation manifest must define at least one group".to_owned());
    }
    let mut previous_name: Option<&str> = None;
    for group in &manifest.groups {
        if !valid_identifier(&group.name) {
            return Err(format!("invalid mutation group name `{}`", group.name));
        }
        if previous_name.is_some_and(|previous| previous >= group.name.as_str()) {
            return Err("mutation group names must be unique and sorted".to_owned());
        }
        previous_name = Some(&group.name);
        let target = find_target(quality, &group.target)?;
        validate_relative_path(&group.product_file)?;
        if !target
            .product_sources
            .iter()
            .any(|root| path_is_within(&group.product_file, root))
        {
            return Err(format!(
                "mutation group `{}` product file `{}` is outside its product source",
                group.name, group.product_file
            ));
        }
        require_regular_file(repository_root, &group.product_file, "product file")?;
        mutation_file(quality, group)?;
        Regex::new(&group.function_regex).map_err(|error| {
            format!(
                "mutation group `{}` has an invalid function regular expression: {error}",
                group.name
            )
        })?;
        if !valid_identifier(&group.test_target) {
            return Err(format!(
                "mutation group `{}` has an invalid test target",
                group.name
            ));
        }
        validate_sorted_paths(repository_root, "mutation test_paths", &group.test_paths)?;
        for path in &group.test_paths {
            if !target
                .test_sources
                .iter()
                .any(|root| path_is_within(path, root))
            {
                return Err(format!(
                    "mutation group `{}` test path `{path}` is outside its test sources",
                    group.name
                ));
            }
        }
        validate_sorted_paths(
            repository_root,
            "mutation group behavior_paths",
            &group.behavior_paths,
        )?;
        for path in &group.behavior_paths {
            if !manifest.behavior_paths.contains(path) {
                return Err(format!(
                    "mutation group `{}` uses undeclared behavior path `{path}`",
                    group.name
                ));
            }
        }
        for exception in &group.exceptions {
            if !matches!(exception.outcome.as_str(), "missed" | "unviable") {
                return Err(format!(
                    "mutation group `{}` exception outcome must be `missed` or `unviable`",
                    group.name
                ));
            }
            Regex::new(&exception.pattern).map_err(|error| {
                format!(
                    "mutation group `{}` has an invalid exception regular expression: {error}",
                    group.name
                )
            })?;
            if exception.reason.trim().is_empty() {
                return Err(format!(
                    "mutation group `{}` exception reason must not be empty",
                    group.name
                ));
            }
        }
    }
    Ok(())
}

fn validate_sorted_paths(
    repository_root: &Path,
    subject: &str,
    paths: &[String],
) -> Result<(), String> {
    let mut previous: Option<&str> = None;
    for path in paths {
        validate_relative_path(path)?;
        if previous.is_some_and(|previous| previous >= path.as_str()) {
            return Err(format!("{subject} must be unique and sorted"));
        }
        previous = Some(path);
        require_regular_file(repository_root, path, subject)?;
    }
    Ok(())
}

fn require_regular_file(repository_root: &Path, path: &str, subject: &str) -> Result<(), String> {
    let path_value = repository_path(repository_root, path)?;
    let metadata = fs::symlink_metadata(&path_value)
        .map_err(|error| format!("could not inspect {subject} `{path}`: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{subject} `{path}` must be a regular file"));
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn read_changed_paths(repository_root: &Path, path: &str) -> Result<Vec<String>, String> {
    let path = repository_path(repository_root, path)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(format!(
                "could not read changed paths `{}`: {error}",
                path.display()
            ));
        }
    };
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err("changed paths must be NUL-terminated".to_owned());
    }
    let mut paths = Vec::new();
    let mut start = 0;
    for end in 0..bytes.len() {
        if bytes[end] != 0 {
            continue;
        }
        let value = &bytes[start..end];
        start = end + 1;
        if value.is_empty() {
            continue;
        }
        let path = match std::str::from_utf8(value) {
            Ok(path) => path,
            Err(_) => return Err("changed paths must contain valid UTF-8".to_owned()),
        };
        validate_relative_path(path)?;
        if paths.contains(&path.to_owned()) {
            return Err(format!("changed paths contain duplicate `{path}`"));
        }
        paths.push(path.to_owned());
    }
    Ok(paths)
}

fn plan_mutations(
    quality: &QualityManifest,
    mutation: &MutationManifest,
    changed: &[String],
    mutants: &[ListedMutant],
) -> Result<MutationMatrix, String> {
    for target in &quality.targets {
        for path in changed {
            if target
                .test_sources
                .iter()
                .any(|root| path_is_within(path, root))
                && !mutation
                    .groups
                    .iter()
                    .any(|group| group.test_paths.contains(path))
            {
                return Err(format!("unmapped test path `{path}`"));
            }
        }
    }
    for path in changed {
        if mutation.behavior_paths.contains(path)
            && !mutation
                .groups
                .iter()
                .any(|group| group.behavior_paths.contains(path))
        {
            return Err(format!("unmapped behavior path `{path}`"));
        }
    }

    let compiled = mutation
        .groups
        .iter()
        .map(|group| Regex::new(&group.function_regex).map(|regex| (group, regex)))
        .collect::<Result<Vec<_>, _>>()
        .expect("mutation regular expressions were validated while loading the manifest");
    let mut selected = BTreeSet::<String>::new();
    let mut product_selected = BTreeSet::<String>::new();
    let mut full_selected = BTreeSet::<String>::new();
    for mutant in mutants {
        let function = mutant
            .function
            .as_ref()
            .map(|function| function.function_name.as_str())
            .unwrap_or("");
        let matching = compiled
            .iter()
            .filter(|(group, regex)| {
                mutation_file(quality, group).is_ok_and(|file| file == mutant.file)
                    && regex.is_match(function)
            })
            .map(|(group, _)| group.name.clone())
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return Err(format!("unmapped mutant `{}`", mutant.name));
        }
        selected.extend(matching.iter().cloned());
        product_selected.extend(matching);
    }
    for group in &mutation.groups {
        if changed
            .iter()
            .any(|path| group.test_paths.contains(path) || group.behavior_paths.contains(path))
        {
            selected.insert(group.name.clone());
            full_selected.insert(group.name.clone());
        }
    }

    let mut include = Vec::new();
    for group in &mutation.groups {
        if !selected.contains(&group.name) {
            continue;
        }
        let target = find_target(quality, &group.target)?;
        include.push(MutationMatrixEntry {
            name: group.name.clone(),
            cargo_manifest: target.cargo_manifest.clone(),
            mutation_file: mutation_file(quality, group)?,
            function_regex: group.function_regex.clone(),
            test_target: group.test_target.clone(),
            use_diff: product_selected.contains(&group.name)
                && !full_selected.contains(&group.name),
        });
    }
    Ok(MutationMatrix { include })
}

fn mutation_file(quality: &QualityManifest, group: &MutationGroup) -> Result<String, String> {
    let target = find_target(quality, &group.target)?;
    let parent = Path::new(&target.cargo_manifest)
        .parent()
        .expect("repository-relative Cargo manifest has a parent");
    let path = match Path::new(&group.product_file).strip_prefix(parent) {
        Ok(path) => path,
        Err(_) => {
            return Err(format!(
                "product file `{}` is outside its Cargo package",
                group.product_file
            ));
        }
    };
    Ok(path.to_string_lossy().into_owned())
}

fn validate_outcomes(
    group: &MutationGroup,
    expected_version: &str,
    outcomes: &MutationOutcomes,
) -> Result<(), String> {
    if outcomes.cargo_mutants_version != expected_version {
        return Err(format!(
            "mutation results use cargo-mutants {}; expected version {expected_version}",
            outcomes.cargo_mutants_version
        ));
    }
    if outcomes.total_mutants
        != outcomes.caught
            + outcomes.missed
            + outcomes.timeout
            + outcomes.unviable
            + outcomes.success
    {
        return Err("mutation result counts are inconsistent".to_owned());
    }
    if outcomes.total_mutants == 0 {
        return Err("mutation results contain no mutants".to_owned());
    }

    let mut baseline_count = 0;
    let mut caught = 0;
    let mut missed = 0;
    let mut unviable = 0;
    for outcome in &outcomes.outcomes {
        if outcome.scenario.as_str() == Some("Baseline") {
            baseline_count += 1;
            if outcome.summary != "Success" {
                return Err(format!(
                    "mutation testing baseline failed with outcome `{}`",
                    outcome.summary
                ));
            }
            continue;
        }
        let Some(mutant) = outcome.scenario.get("Mutant") else {
            return Err("mutation outcomes contain a malformed mutant scenario".to_owned());
        };
        let Some(mutant_name) = mutant.get("name").and_then(serde_json::Value::as_str) else {
            return Err("mutation outcomes contain a malformed mutant scenario".to_owned());
        };
        let exception_outcome = match outcome.summary.as_str() {
            "CaughtMutant" => {
                caught += 1;
                None
            }
            "MissedMutant" => {
                missed += 1;
                Some("missed")
            }
            "Timeout" => {
                return Err(format!(
                    "mutation results contain a timed out mutant `{mutant_name}`"
                ));
            }
            "Unviable" => {
                unviable += 1;
                Some("unviable")
            }
            summary => {
                return Err(format!(
                    "mutation results contain unknown mutation outcome `{summary}`"
                ));
            }
        };
        let Some(exception_outcome) = exception_outcome else {
            continue;
        };
        let mut approved = false;
        for exception in &group.exceptions {
            let pattern = Regex::new(&exception.pattern)
                .expect("exception regular expressions were validated while loading the manifest");
            if exception.outcome == exception_outcome && pattern.is_match(mutant_name) {
                approved = true;
                break;
            }
        }
        if !approved {
            return Err(format!(
                "mutation results contain an unapproved {exception_outcome} mutant `{mutant_name}`"
            ));
        }
    }
    if baseline_count != 1 {
        return Err("mutation results must contain exactly one successful baseline".to_owned());
    }
    let observed = (caught, missed, 0, unviable, 0, caught + missed + unviable);
    let reported = (
        outcomes.caught,
        outcomes.missed,
        outcomes.timeout,
        outcomes.unviable,
        outcomes.success,
        outcomes.total_mutants,
    );
    if observed != reported {
        return Err("mutation result counts are inconsistent with outcomes".to_owned());
    }
    Ok(())
}

fn verify_tools(repository_root: &Path, tools: &Tools) -> Result<(), String> {
    verify_version(
        repository_root,
        "rustc",
        &["--version"],
        &format!("rustc {}", tools.rust),
    )?;
    verify_version(
        repository_root,
        "cargo llvm-cov",
        &["llvm-cov", "--version"],
        &format!("cargo-llvm-cov {}", tools.cargo_llvm_cov),
    )
}

fn verify_version(
    repository_root: &Path,
    tool: &str,
    arguments: &[&str],
    expected_prefix: &str,
) -> Result<(), String> {
    let program = if tool == "rustc" { "rustc" } else { "cargo" };
    let output = Command::new(program)
        .args(arguments)
        .current_dir(repository_root)
        .output()
        .map_err(|error| format!("required tool `{tool}` is unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "required tool `{tool}` could not report its version: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let version = match std::str::from_utf8(&output.stdout) {
        Ok(version) => version.trim(),
        Err(_) => {
            return Err(format!(
                "required tool `{tool}` returned a non-UTF-8 version"
            ));
        }
    };
    let expected_version = version == expected_prefix
        || version
            .strip_prefix(expected_prefix)
            .is_some_and(|suffix| suffix.starts_with(' '));
    if !expected_version {
        return Err(format!(
            "required tool `{tool}` has version `{version}`; expected `{expected_prefix}`"
        ));
    }
    Ok(())
}

fn run_command(
    repository_root: &Path,
    gate: &str,
    program: &str,
    arguments: &[&str],
) -> Result<(), String> {
    let status = match Command::new(program)
        .args(arguments)
        .current_dir(repository_root)
        .status()
    {
        Ok(status) => status,
        Err(error) => return Err(format!("could not run {gate}: {error}")),
    };
    if status.success() {
        Ok(())
    } else {
        let status = match status.code() {
            Some(code) => code.to_string(),
            None => "signal".to_owned(),
        };
        Err(format!("{gate} failed with status {status}"))
    }
}

fn find_target<'a>(manifest: &'a QualityManifest, name: &str) -> Result<&'a Target, String> {
    for target in &manifest.targets {
        if target.name == name {
            return Ok(target);
        }
    }
    Err(format!("unknown quality target `{name}`"))
}

fn repository_path(repository_root: &Path, path: &str) -> Result<PathBuf, String> {
    validate_relative_path(path)?;
    let joined = repository_root.join(path);
    if let Ok(canonical) = fs::canonicalize(&joined)
        && !canonical.starts_with(repository_root)
    {
        return Err(format!("path `{path}` resolves outside the repository"));
    }
    Ok(joined)
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let path_value = Path::new(path);
    if path.is_empty()
        || path_value.is_absolute()
        || path_value.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().is_empty()
        })
    {
        return Err(format!(
            "quality paths must be non-empty repository-relative paths: `{path}`"
        ));
    }
    Ok(())
}

fn paths_overlap(left: &str, right: &str) -> bool {
    path_is_within(left, right) || path_is_within(right, left)
}

fn path_is_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn cargo_manifest_parent(repository_root: &Path, target: &Target) -> Result<PathBuf, String> {
    let manifest = repository_path(repository_root, &target.cargo_manifest)?;
    Ok(manifest
        .parent()
        .expect("a repository-relative manifest always has a parent")
        .to_path_buf())
}

fn coverage_exclusion_regex(target: &Target) -> String {
    target
        .test_sources
        .iter()
        .chain(&target.generated)
        .chain(&target.third_party)
        .map(|path| regex_escape(&format!("/{path}/")))
        .collect::<Vec<_>>()
        .join("|")
}

fn regex_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(
            character,
            '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
