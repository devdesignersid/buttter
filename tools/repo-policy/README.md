# Repository Policy CLI

`repo-policy` enforces the objective pull request body and commit-message requirements defined by this repository. It gives contributors immediate local diagnostics and gives CI a deterministic pass-or-fail result.

The tool does not judge subjective requirements such as readability, design quality, appropriate scope, or whether evidence is truthful. Those requirements still need human review.

## Prerequisites

- Rust and Cargo with Rust 2024 edition support. CI uses Rust 1.98.1.
- [GitHub CLI](https://cli.github.com/) authenticated for the target repository when using `--github-repository`. Structural validation does not require GitHub CLI or network access.

The Rust crate has no third-party runtime dependencies.

## Usage

From the repository root, validate a body stored in a file:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-body path/to/body.md
```

Validate standard input:

```sh
cat path/to/body.md | \
  cargo run --manifest-path tools/repo-policy/Cargo.toml -- validate-pr-body
```

Also verify that the related issue is open and belongs to a specific repository:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-body --github-repository devdesignersid/buttter path/to/body.md
```

For CI, include the pull request number to enforce work-item readiness and approval:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-body \
    --github-repository devdesignersid/buttter \
    --pull-request-number 10 \
    path/to/body.md
```

Validate a commit-message file, as the repository-managed `commit-msg` hook does:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-commit-message path/to/COMMIT_EDITMSG
```

Omit the path to read a message from standard input. Validate every commit in a pull request through the authenticated GitHub CLI:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-commits \
    --github-repository devdesignersid/buttter \
    --pull-request-number 10
```

Print built-in help:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- --help
```

A successful invocation identifies the policy that passed and exits with status 0. Invalid arguments, unreadable or non-UTF-8 input, policy violations, GitHub CLI failures, API failures, and malformed GitHub data produce diagnostics on standard error and exit with status 1.

## Commit-message contract

Messages use this Conventional Commits 1.0.0 grammar:

```text
<type>[(scope)][!]: <description>

[optional body]

[optional trailers]
```

The allowed types are `build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, and `test`. Extending this list is a repository policy change. A scope is non-empty text inside one pair of parentheses and cannot contain parentheses or control characters. A non-empty description follows a colon and space. `!` immediately before the colon marks a breaking change.

A body is free-form UTF-8 text separated from the subject by a blank line. LF and CRLF line endings are accepted; bare carriage returns are rejected.

A trailing footer paragraph uses Git-style trailers. Each trailer starts with an alphanumeric-or-hyphen token and uses `Token: value` or `Token #value`; indented lines continue the preceding trailer. `BREAKING CHANGE: description` and `BREAKING-CHANGE: description` are accepted. Trailer values cannot be empty.

Subjects beginning with `Merge ` are exempt as Git-generated merge messages. Reverts otherwise use the normal grammar, such as `revert: restore previous behavior`; legacy `Revert "..."` subjects are not exempt.

The `.githooks/commit-msg` hook passes Git's message file to `validate-commit-message`. Configure clones with `git config core.hooksPath .githooks`, as required by `AGENTS.md`.

`validate-pr-commits` requests every pull-request commit with pagination and verifies the unique record count against pull-request metadata. GitHub API version `2022-11-28` limits this endpoint to 250 commits, so larger pull requests fail closed. A commit is exempt when GitHub attributes it to a non-empty login ending in `[bot]`. Names or email addresses embedded in Git commit metadata do not establish that exemption. Missing or duplicate commits, malformed records, invalid encoding, API errors, and nonconforming non-bot messages fail closed; diagnostics identify the rejected commit SHA.

## Enforced body contract

HTML comments are removed before validation, so instructional comments in `.github/pull_request_template.md` do not satisfy required fields.

### Headings

These level-two and level-three headings must each occur exactly once and in this order. No additional headings at either level are accepted.

1. `## Related Issue`
2. `## Acceptance Criteria`
3. `## Scope`
4. `### Included`
5. `### Excluded`
6. `## Evidence`
7. `## Executed Quality Gates`
8. `## Human Line Review`
9. `## Unverified Items`

### Related issue

The section must contain exactly one line in this form:

```md
- Closes #6
```

The number must be greater than zero. When `--github-repository OWNER/REPO` is supplied, the tool invokes `gh api` using GitHub REST API version `2022-11-28` and fails unless the number identifies an open issue. Closed issues, pull requests, missing issues, authentication failures, rate limits, network failures, unavailable GitHub CLI installations, and unexpected API responses fail closed.

When `--pull-request-number` is supplied, it must be a positive integer and requires `--github-repository`. The linked issue must also pass the work-item and approval contracts below.

### Linked work item

These level-three headings must each occur exactly once and in this order, matching `.github/ISSUE_TEMPLATE/work-item.yml`:

1. `### Problem`
2. `### Desired observable outcome`
3. `### Acceptance criteria`
4. `### Non-goals`
5. `### Decisions needed`
6. `### Evidence plan`
7. `### Applicable quality gates`
8. `### Review scope`

Every section must contain visible content after HTML comments are removed. Empty bullets and empty checklist items are placeholders and do not count. Acceptance criteria must consist of one or more non-empty checked or unchecked checklist items. Non-goals may be exactly `None`; unresolved decisions are rejected, so Decisions needed must be exactly `None`. The validator checks structure, not whether the stated requirements or criteria are substantively good or testable.

### Implementation approval

The linked issue must currently have the `implementation-approved` label. The latest matching `labeled` or `unlabeled` issue timeline event must:

- be a `labeled` event;
- identify `devdesignersid` as the actor; and
- have a GitHub `created_at` timestamp strictly earlier than the pull request's GitHub `created_at` timestamp.

The timeline request is paginated. Missing, malformed, removed, unauthorized, equal-time, or late approval records fail closed, as do all CLI, API, authentication, rate-limit, network, UTF-8, and response errors. Removing and validly reapplying the label creates a new approval record; only the latest event controls.

Pull-request creation is the defined implementation boundary because Git author and committer timestamps are contributor-controlled. This policy does not prove when local work began. A GitHub identity is also not independent human approval when an agent can use the same credentials; the allowlist proves only which credential applied the label.

### Acceptance criteria

The section must contain one or more non-empty checked items and no other visible content:

```md
- [x] A concrete criterion.
```

Both `[x]` and `[X]` are accepted. Unchecked or empty items are rejected.

### Included and excluded scope

Each subsection must contain one or more non-empty bullet items. When there are no exclusions, state why:

```md
- None: no additional behavior is excluded.
```

### Evidence

All three fields must appear in template order and contain values:

```md
- Repository evidence: `path/to/file:10-20`.
- External evidence: Not applicable: the behavior is repository-defined.
- Performance evidence or not applicable: Not applicable: performance is unchanged.
```

Use `Not applicable: <reason>` instead of leaving an inapplicable field empty.

### Executed quality gates

The template table header and separator must remain unchanged. At least one non-empty command row is required, and every result must be exactly `PASS`:

```md
| Gate or command | Result |
| --- | --- |
| `cargo test` | PASS |
```

### Human line review

The review checkbox must be checked with its template text unchanged. Every following field must contain a value:

```md
- [x] The user has read every line of repository-authored changes.
- Generated files excluded from line review: `Cargo.lock`.
- Third-party source excluded from line review: None.
- Concerns requiring additional review: None.
```

Use `None` when a field has no items.

### Unverified items

Provide one or more non-empty bullet items. Use `- None` when nothing remains unverified.

## CI integration and trust boundary

`.github/workflows/pr-body-policy.yml` runs for pull request creation, edits, reopening, synchronization, and transitions to ready for review. Draft pull requests may remain failing while incomplete; ready-for-review pull requests are expected to pass.

The workflow uses `pull_request_target`, checks out the pull request's base commit, and executes only trusted policy code from that commit. It does not check out or execute code from the pull request. Its token has only `contents: read`, `issues: read`, and `pull-requests: read` permissions. The pull request body is passed as data through an environment variable rather than evaluated as shell code.

`.github/workflows/commit-message-policy.yml` also uses `pull_request_target` and trusted policy code from the pull request's base commit. It reads paginated commit metadata through the GitHub API and never checks out or executes pull-request code. Its token has only `contents: read` and `pull-requests: read` permissions.

`.github/workflows/repo-policy-quality.yml` runs formatting, Clippy, tests, and the 100% line-coverage gate when the policy tool, commit hook, or workflows change. That workflow uses the pull request code but has read-only repository permissions and does not request or use repository secrets.

Policy jobs have explicit five-minute timeouts, and the quality job has a ten-minute timeout. These bounds fail closed on hangs while leaving substantial margin over observed execution times.

### Server-side enforcement

The `PR body policy` and `Commit message policy` status checks are required on `main`; otherwise CI would only detect and report violations.

Online issue verification reflects the issue state when the workflow runs. If an issue is closed without another supported pull request event, rerun the workflow before relying on the previous result.

## Development

Run every local quality gate from the repository root:

```sh
cargo fmt --manifest-path tools/repo-policy/Cargo.toml -- --check
cargo clippy --manifest-path tools/repo-policy/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path tools/repo-policy/Cargo.toml
cargo llvm-cov --manifest-path tools/repo-policy/Cargo.toml --fail-under-lines 100
```

`cargo-llvm-cov` 0.9.1 is used by CI. Install that version when reproducing the coverage gate locally:

```sh
cargo install cargo-llvm-cov --version 0.9.1 --locked
```

## Maintaining the policy

When a pull request, work-item, or commit-message contract changes:

1. Update the applicable template, hook, workflow, or grammar documentation.
2. Add or change tests under `tools/repo-policy/tests` and confirm they fail for the expected reason.
3. Make the minimum validator change in `tools/repo-policy/src/main.rs`.
4. Update this document and built-in help when commands or behavior change.
5. Run all development quality gates, including 100% line coverage.
6. Review workflow permissions and the trusted-code boundary if CI behavior changes.
7. Confirm the GitHub ruleset still requires both policy checks.

Keep template requirements, validator behavior, tests, and documentation in the same independently deployable change so they cannot drift silently. Changes to `APPROVAL_LABEL` or `AUTHORIZED_APPROVERS` in `src/main.rs` are policy changes and require the same review.
