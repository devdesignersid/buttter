use serde::Deserialize;
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
}

#[derive(Deserialize)]
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
    manifest
        .targets
        .iter()
        .find(|target| target.name == name)
        .ok_or_else(|| format!("unknown quality target `{name}`"))
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
