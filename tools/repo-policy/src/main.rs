use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::{self, Command};

const USAGE: &str = "Repository pull request policy validator.\n\nUsage:\n  repo-policy validate-pr-body [--github-repository OWNER/REPO] [BODY_FILE]\n\nCommands:\n  validate-pr-body  Validate the required pull request body structure and content.\n\nOptions:\n  --github-repository OWNER/REPO\n      Use the authenticated GitHub CLI to verify that `Closes #N` identifies an open issue.\n  -h, --help\n      Print this help.\n\nInput:\n  Reads BODY_FILE when supplied; otherwise reads the pull request body from standard input.\n\nExit status:\n  0  The body and any requested issue verification passed.\n  1  Arguments, input, body content, or issue verification failed.";

const HEADINGS: [&str; 9] = [
    "## Related Issue",
    "## Acceptance Criteria",
    "## Scope",
    "### Included",
    "### Excluded",
    "## Evidence",
    "## Executed Quality Gates",
    "## Human Line Review",
    "## Unverified Items",
];

struct ValidatedBody {
    issue_number: u64,
}

fn main() {
    match run(env::args().skip(1).collect()) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("{message}");
            process::exit(1);
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    if matches!(arguments.as_slice(), [argument] if argument == "--help" || argument == "-h") {
        println!("{USAGE}");
        return Ok(());
    }

    let Some((command, command_arguments)) = arguments.split_first() else {
        return Err(USAGE.to_owned());
    };
    if command != "validate-pr-body" {
        return Err(USAGE.to_owned());
    }

    let (repository, body_path) = parse_validate_arguments(command_arguments)?;
    let body = read_body(body_path.as_deref())?;
    let validated = validate_body(&body).map_err(|violations| {
        format!(
            "Pull request body policy violations:\n- {}",
            violations.join("\n- ")
        )
    })?;

    if let Some(repository) = repository {
        verify_open_issue(&repository, validated.issue_number)?;
    }

    println!("Pull request body is valid.");
    Ok(())
}

fn parse_validate_arguments(
    arguments: &[String],
) -> Result<(Option<String>, Option<String>), String> {
    let mut repository = None;
    let mut body_path = None;
    let mut index = 0;

    while index < arguments.len() {
        if arguments[index] == "--github-repository" {
            index += 1;
            let Some(value) = arguments.get(index) else {
                return Err(USAGE.to_owned());
            };
            if repository.is_some() || !valid_repository(value) {
                return Err(USAGE.to_owned());
            }
            repository = Some(value.clone());
        } else if arguments[index] == "--help" || arguments[index] == "-h" || body_path.is_some() {
            return Err(USAGE.to_owned());
        } else {
            body_path = Some(arguments[index].clone());
        }
        index += 1;
    }

    Ok((repository, body_path))
}

fn valid_repository(repository: &str) -> bool {
    let mut components = repository.split('/');
    let valid_component = |component: &str| {
        !component.is_empty()
            && component
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
    };

    matches!(
        (components.next(), components.next(), components.next()),
        (Some(owner), Some(name), None) if valid_component(owner) && valid_component(name)
    )
}

fn read_body(path: Option<&str>) -> Result<String, String> {
    let bytes = if let Some(path) = path {
        fs::read(path).map_err(|error| format!("could not read `{path}`: {error}"))?
    } else {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read standard input: {error}"))?;
        bytes
    };

    String::from_utf8(bytes).map_err(|_| "pull request body must be valid UTF-8".to_owned())
}

fn validate_body(body: &str) -> Result<ValidatedBody, Vec<String>> {
    let visible_body = remove_html_comments(body);
    let lines: Vec<&str> = visible_body.lines().collect();
    let headings: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| is_level_two_or_three_heading(line))
        .collect();

    if headings != HEADINGS {
        return Err(vec![format!(
            "required headings must occur exactly once and in this order: {}",
            HEADINGS.join(", ")
        )]);
    }

    let boundaries: Vec<usize> = HEADINGS
        .iter()
        .map(|heading| {
            lines
                .iter()
                .position(|line| line == heading)
                .expect("validated heading must have a boundary")
        })
        .collect();
    let section = |heading_index: usize| {
        let start = boundaries[heading_index] + 1;
        let end = boundaries
            .get(heading_index + 1)
            .copied()
            .unwrap_or(lines.len());
        meaningful_lines(&lines[start..end])
    };

    let mut violations = Vec::new();
    let issue_number = validate_related_issue(&section(0), &mut violations);
    validate_checklist("Acceptance Criteria", &section(1), &mut violations);
    validate_bullet_list("Included", &section(3), &mut violations);
    validate_bullet_list("Excluded", &section(4), &mut violations);
    validate_labeled_lines(
        "Evidence",
        &section(5),
        &[
            "- Repository evidence:",
            "- External evidence:",
            "- Performance evidence or not applicable:",
        ],
        &mut violations,
    );
    validate_quality_gates(&section(6), &mut violations);
    validate_human_review(&section(7), &mut violations);
    validate_bullet_list("Unverified Items", &section(8), &mut violations);

    if violations.is_empty() {
        Ok(ValidatedBody {
            issue_number: issue_number.expect("valid body must have an issue number"),
        })
    } else {
        Err(violations)
    }
}

fn remove_html_comments(body: &str) -> String {
    let mut visible = String::new();
    let mut remaining = body;

    while let Some(comment_start) = remaining.find("<!--") {
        visible.push_str(&remaining[..comment_start]);
        let after_start = &remaining[comment_start + 4..];
        let Some(comment_end) = after_start.find("-->") else {
            return visible;
        };
        remaining = &after_start[comment_end + 3..];
    }
    visible.push_str(remaining);
    visible
}

fn is_level_two_or_three_heading(line: &str) -> bool {
    let Some(after_hashes) = line.strip_prefix("##") else {
        return false;
    };
    after_hashes.starts_with(' ') || after_hashes.starts_with("# ")
}

fn meaningful_lines<'a>(lines: &'a [&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect()
}

fn validate_related_issue(lines: &[&str], violations: &mut Vec<String>) -> Option<u64> {
    let issue_number = match lines {
        [line] => line
            .strip_prefix("- Closes #")
            .filter(|number| {
                !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
            })
            .and_then(|number| number.parse::<u64>().ok())
            .filter(|number| *number > 0),
        _ => None,
    };

    if issue_number.is_none() {
        violations
            .push("Related Issue must contain exactly `- Closes #<positive-number>`".to_owned());
    }
    issue_number
}

fn validate_checklist(section: &str, lines: &[&str], violations: &mut Vec<String>) {
    let valid = !lines.is_empty()
        && lines.iter().all(|line| {
            line.strip_prefix("- [x] ")
                .or_else(|| line.strip_prefix("- [X] "))
                .is_some_and(|item| !item.trim().is_empty())
        });
    if !valid {
        violations.push(format!(
            "{section} must contain one or more non-empty checked items and no other content"
        ));
    }
}

fn validate_bullet_list(section: &str, lines: &[&str], violations: &mut Vec<String>) {
    let valid = !lines.is_empty()
        && lines.iter().all(|line| {
            line.strip_prefix("- ")
                .is_some_and(|item| !item.trim().is_empty())
        });
    if !valid {
        violations.push(format!(
            "{section} must contain one or more non-empty bullet items and no other content"
        ));
    }
}

fn validate_labeled_lines(
    section: &str,
    lines: &[&str],
    labels: &[&str],
    violations: &mut Vec<String>,
) {
    let valid = lines.len() == labels.len()
        && lines.iter().zip(labels).all(|(line, label)| {
            line.strip_prefix(label)
                .is_some_and(|value| !value.trim().is_empty())
        });
    if !valid {
        violations.push(format!(
            "{section} must complete every required field in template order"
        ));
    }
}

fn validate_quality_gates(lines: &[&str], violations: &mut Vec<String>) {
    let valid_header = lines.first() == Some(&"| Gate or command | Result |")
        && lines.get(1) == Some(&"| --- | --- |");
    let rows = lines.get(2..).unwrap_or_default();
    let valid_rows = !rows.is_empty() && rows.iter().all(|row| valid_quality_gate_row(row));

    if !valid_header || !valid_rows {
        violations.push(
            "Executed Quality Gates must use the template table and contain one or more non-empty `PASS` rows"
                .to_owned(),
        );
    }
}

fn valid_quality_gate_row(row: &str) -> bool {
    let columns: Vec<&str> = row.split('|').collect();
    matches!(
        columns.as_slice(),
        [before, command, result, after]
            if before.trim().is_empty()
                && !command.trim().is_empty()
                && result.trim() == "PASS"
                && after.trim().is_empty()
    )
}

fn validate_human_review(lines: &[&str], violations: &mut Vec<String>) {
    const CHECKBOX: &str = "- [x] The user has read every line of repository-authored changes.";
    const LABELS: [&str; 3] = [
        "- Generated files excluded from line review:",
        "- Third-party source excluded from line review:",
        "- Concerns requiring additional review:",
    ];

    let valid = lines.first().is_some_and(|line| {
        *line == CHECKBOX
            || *line == "- [X] The user has read every line of repository-authored changes."
    }) && lines.len() == LABELS.len() + 1
        && lines[1..].iter().zip(LABELS).all(|(line, label)| {
            line.strip_prefix(label)
                .is_some_and(|value| !value.trim().is_empty())
        });

    if !valid {
        violations.push(
            "Human Line Review must be checked and complete every required field in template order"
                .to_owned(),
        );
    }
}

fn verify_open_issue(repository: &str, issue_number: u64) -> Result<(), String> {
    let endpoint = format!("repos/{repository}/issues/{issue_number}");
    let output = Command::new("gh")
        .args([
            "api",
            &endpoint,
            "--jq",
            "if has(\"pull_request\") then \"pull_request\" else .state end",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
        ])
        .output()
        .map_err(|error| format!("could not start GitHub CLI to verify related issue: {error}"))?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "GitHub API could not verify related issue #{issue_number}: {}",
            detail.trim()
        ));
    }

    let response = String::from_utf8(output.stdout)
        .map_err(|_| "GitHub API issue response must be valid UTF-8".to_owned())?;
    match response.trim() {
        "open" => Ok(()),
        "closed" => Err(format!("related issue #{issue_number} is not open")),
        "pull_request" => Err(format!(
            "related reference #{issue_number} identifies a pull request, not an issue"
        )),
        response => Err(format!(
            "GitHub API returned an unexpected response for related issue #{issue_number}: `{response}`"
        )),
    }
}
