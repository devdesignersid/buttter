use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::{self, Command};

const USAGE: &str = "Repository pull request policy validator.\n\nUsage:\n  repo-policy validate-pr-body [--github-repository OWNER/REPO] [--pull-request-number NUMBER] [BODY_FILE]\n\nCommands:\n  validate-pr-body  Validate the required pull request body and linked work item.\n\nOptions:\n  --github-repository OWNER/REPO\n      Use the authenticated GitHub CLI to verify the linked issue.\n  --pull-request-number NUMBER\n      Also verify work-item readiness and approval before pull request creation.\n  -h, --help\n      Print this help.\n\nInput:\n  Reads BODY_FILE when supplied; otherwise reads the pull request body from standard input.\n\nExit status:\n  0  The body and any requested GitHub verification passed.\n  1  Arguments, input, policy, or GitHub verification failed.";

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

const WORK_ITEM_HEADINGS: [&str; 8] = [
    "### Problem",
    "### Desired observable outcome",
    "### Acceptance criteria",
    "### Non-goals",
    "### Decisions needed",
    "### Evidence plan",
    "### Applicable quality gates",
    "### Review scope",
];
const APPROVAL_LABEL: &str = "implementation-approved";
const AUTHORIZED_APPROVERS: [&str; 1] = ["devdesignersid"];

struct ValidateArguments {
    repository: Option<String>,
    pull_request_number: Option<u64>,
    body_path: Option<String>,
}

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

    let arguments = parse_validate_arguments(command_arguments)?;
    let body = read_body(arguments.body_path.as_deref())?;
    let validated = validate_body(&body).map_err(|violations| {
        format!(
            "Pull request body policy violations:\n- {}",
            violations.join("\n- ")
        )
    })?;

    if let Some(repository) = arguments.repository {
        verify_open_issue(&repository, validated.issue_number)?;
        if let Some(pull_request_number) = arguments.pull_request_number {
            verify_work_item_readiness(&repository, validated.issue_number, pull_request_number)?;
        }
    }

    println!("Pull request body is valid.");
    Ok(())
}

fn parse_validate_arguments(arguments: &[String]) -> Result<ValidateArguments, String> {
    let mut repository = None;
    let mut pull_request_number = None;
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
        } else if arguments[index] == "--pull-request-number" {
            index += 1;
            let Some(value) = arguments.get(index) else {
                return Err(USAGE.to_owned());
            };
            let number = value.parse::<u64>().ok().filter(|number| *number > 0);
            if pull_request_number.is_some() || number.is_none() {
                return Err(USAGE.to_owned());
            }
            pull_request_number = number;
        } else if arguments[index] == "--help" || arguments[index] == "-h" || body_path.is_some() {
            return Err(USAGE.to_owned());
        } else {
            body_path = Some(arguments[index].clone());
        }
        index += 1;
    }

    if pull_request_number.is_some() && repository.is_none() {
        return Err(USAGE.to_owned());
    }

    Ok(ValidateArguments {
        repository,
        pull_request_number,
        body_path,
    })
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
    let response = github_api(
        &endpoint,
        "if has(\"pull_request\") then \"pull_request\" else .state end",
        false,
        &format!("related issue #{issue_number}"),
    )?;
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

fn github_api(
    endpoint: &str,
    query: &str,
    paginate: bool,
    subject: &str,
) -> Result<String, String> {
    let mut command = Command::new("gh");
    command.args(["api", endpoint, "--jq", query]);
    if paginate {
        command.arg("--paginate");
    }
    let output = command
        .args(["-H", "X-GitHub-Api-Version: 2022-11-28"])
        .output()
        .map_err(|error| format!("could not start GitHub CLI to verify {subject}: {error}"))?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "GitHub API could not verify {subject}: {}",
            detail.trim()
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|_| format!("GitHub API response for {subject} must be valid UTF-8"))
}

fn verify_work_item_readiness(
    repository: &str,
    issue_number: u64,
    pull_request_number: u64,
) -> Result<(), String> {
    let issue_endpoint = format!("repos/{repository}/issues/{issue_number}");
    let issue_body = github_api(
        &issue_endpoint,
        ".body",
        false,
        &format!("work item #{issue_number}"),
    )?;
    validate_work_item(&issue_body).map_err(|violations| {
        format!(
            "Linked work-item policy violations:\n- {}",
            violations.join("\n- ")
        )
    })?;

    let current_label = github_api(
        &issue_endpoint,
        &format!("[.labels[].name] | index(\"{APPROVAL_LABEL}\") != null"),
        false,
        &format!("approval label on issue #{issue_number}"),
    )?;
    match current_label.trim() {
        "true" => {}
        "false" => {
            return Err(format!(
                "related issue #{issue_number} does not currently have the `{APPROVAL_LABEL}` label"
            ));
        }
        response => {
            return Err(format!(
                "GitHub API returned an unexpected response for approval label on issue #{issue_number}: `{response}`"
            ));
        }
    }

    let timeline_endpoint = format!("{issue_endpoint}/timeline");
    let timeline = github_api(
        &timeline_endpoint,
        &format!(
            ".[] | select((.event == \"labeled\" or .event == \"unlabeled\") and .label.name == \"{APPROVAL_LABEL}\") | [.event, .created_at, .actor.login] | @tsv"
        ),
        true,
        &format!("approval timeline for issue #{issue_number}"),
    )?;
    let latest = parse_latest_approval_event(&timeline)?;
    if latest.kind != "labeled" {
        return Err("latest approval-label event is not an application".to_owned());
    }
    if !AUTHORIZED_APPROVERS
        .iter()
        .any(|approver| latest.actor.eq_ignore_ascii_case(approver))
    {
        return Err(format!(
            "GitHub identity `{}` is not authorized to approve implementation",
            latest.actor
        ));
    }

    let pull_endpoint = format!("repos/{repository}/pulls/{pull_request_number}");
    let created_at = github_api(
        &pull_endpoint,
        ".created_at",
        false,
        &format!("pull request #{pull_request_number}"),
    )?;
    let pull_created_at = Timestamp::parse(created_at.trim())
        .ok_or_else(|| "GitHub API returned a malformed pull request timestamp".to_owned())?;
    if latest.timestamp >= pull_created_at {
        return Err(format!(
            "the authorized `{APPROVAL_LABEL}` event must predate pull request creation"
        ));
    }

    Ok(())
}

fn validate_work_item(body: &str) -> Result<(), Vec<String>> {
    let visible_body = remove_html_comments(body);
    let lines: Vec<&str> = visible_body.lines().collect();
    let headings: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| line.starts_with("### "))
        .collect();
    if headings != WORK_ITEM_HEADINGS {
        return Err(vec![format!(
            "required work-item headings must occur exactly once and in this order: {}",
            WORK_ITEM_HEADINGS.join(", ")
        )]);
    }

    let boundaries: Vec<usize> = WORK_ITEM_HEADINGS
        .iter()
        .map(|heading| {
            lines
                .iter()
                .position(|line| line == heading)
                .expect("validated work-item heading must have a boundary")
        })
        .collect();
    let section = |index: usize| {
        let start = boundaries[index] + 1;
        let end = boundaries.get(index + 1).copied().unwrap_or(lines.len());
        meaningful_lines(&lines[start..end])
    };

    let mut violations = Vec::new();
    validate_work_item_text("Problem", &section(0), &mut violations);
    validate_work_item_text("Desired observable outcome", &section(1), &mut violations);
    validate_work_item_criteria(&section(2), &mut violations);
    validate_work_item_text("Non-goals", &section(3), &mut violations);
    if section(4) != ["None"] {
        violations.push("Decisions needed must be exactly `None`".to_owned());
    }
    validate_work_item_text("Evidence plan", &section(5), &mut violations);
    validate_work_item_text("Applicable quality gates", &section(6), &mut violations);
    validate_work_item_text("Review scope", &section(7), &mut violations);

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn validate_work_item_text(section: &str, lines: &[&str], violations: &mut Vec<String>) {
    if lines.is_empty() || lines.iter().all(|line| is_placeholder(line)) {
        violations.push(format!("{section} must contain non-placeholder content"));
    }
}

fn is_placeholder(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line == "-" || ["- [ ]", "- [x]", "- [X]"].contains(&line)
}

fn validate_work_item_criteria(lines: &[&str], violations: &mut Vec<String>) {
    let valid = !lines.is_empty()
        && lines.iter().all(|line| {
            line.strip_prefix("- [ ] ")
                .or_else(|| line.strip_prefix("- [x] "))
                .or_else(|| line.strip_prefix("- [X] "))
                .is_some_and(|criterion| !criterion.trim().is_empty())
        });
    if !valid {
        violations.push(
            "Acceptance criteria must contain one or more non-empty checklist items and no other content"
                .to_owned(),
        );
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Timestamp {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl Timestamp {
    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 20
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes[19] != b'Z'
        {
            return None;
        }
        let number = |start: usize, end: usize| {
            value[start..end]
                .chars()
                .all(|character| character.is_ascii_digit())
                .then(|| value[start..end].parse::<u16>().ok())
                .flatten()
        };
        let timestamp = Self {
            year: number(0, 4)?,
            month: u8::try_from(number(5, 7)?).ok()?,
            day: u8::try_from(number(8, 10)?).ok()?,
            hour: u8::try_from(number(11, 13)?).ok()?,
            minute: u8::try_from(number(14, 16)?).ok()?,
            second: u8::try_from(number(17, 19)?).ok()?,
        };
        let leap_year = timestamp.year.is_multiple_of(4)
            && (!timestamp.year.is_multiple_of(100) || timestamp.year.is_multiple_of(400));
        let days = match timestamp.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap_year => 29,
            2 => 28,
            _ => return None,
        };
        (timestamp.day > 0
            && timestamp.day <= days
            && timestamp.hour < 24
            && timestamp.minute < 60
            && timestamp.second < 60)
            .then_some(timestamp)
    }
}

struct ApprovalEvent<'a> {
    kind: &'a str,
    timestamp: Timestamp,
    actor: &'a str,
}

fn parse_latest_approval_event(timeline: &str) -> Result<ApprovalEvent<'_>, String> {
    let mut events = Vec::new();
    for line in timeline.lines() {
        let columns: Vec<&str> = line.split('\t').collect();
        let [kind @ ("labeled" | "unlabeled"), timestamp, actor] = columns.as_slice() else {
            return Err("GitHub API returned a malformed approval timeline".to_owned());
        };
        let Some(timestamp) = Timestamp::parse(timestamp) else {
            return Err("GitHub API returned a malformed approval timeline".to_owned());
        };
        if actor.is_empty() {
            return Err("GitHub API returned a malformed approval timeline".to_owned());
        }
        events.push(ApprovalEvent {
            kind,
            timestamp,
            actor,
        });
    }
    events
        .into_iter()
        .max_by_key(|event| event.timestamp)
        .ok_or_else(|| "GitHub API returned an empty approval timeline".to_owned())
}
