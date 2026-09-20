# Repository Policy CLI

`repo-policy` enforces the objective pull request body requirements defined by this repository. It gives contributors immediate local diagnostics and gives CI a deterministic pass-or-fail result.

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

Print built-in help:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- --help
```

A successful invocation writes `Pull request body is valid.` and exits with status 0. Invalid arguments, unreadable or non-UTF-8 input, policy violations, GitHub CLI failures, API failures, and invalid issue states produce diagnostics on standard error and exit with status 1.

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

The workflow uses `pull_request_target`, checks out the pull request's base commit, and executes only trusted policy code from that commit. It does not check out or execute code from the pull request. Its token has only `contents: read` and `issues: read` permissions. The pull request body is passed as data through an environment variable rather than evaluated as shell code.

`.github/workflows/repo-policy-quality.yml` runs formatting, Clippy, tests, and the 100% line-coverage gate when the policy tool or its workflows change. That workflow uses the pull request code but has read-only repository permissions and does not request or use repository secrets.

### Server-side enforcement

The `PR body policy` status check must be required by a GitHub ruleset on `main`; otherwise CI only detects and reports violations. The initial workflow is a bootstrap exception because a trusted `pull_request_target` workflow cannot run from the default branch until it has been merged there. Configure the required check immediately after that bootstrap merge.

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

When the pull request contract changes:

1. Update `.github/pull_request_template.md`.
2. Add or change tests under `tools/repo-policy/tests` and confirm they fail for the expected reason.
3. Make the minimum validator change in `tools/repo-policy/src/main.rs`.
4. Update this document and built-in help when commands or behavior change.
5. Run all development quality gates, including 100% line coverage.
6. Review workflow permissions and the trusted-code boundary if CI behavior changes.
7. Confirm the GitHub ruleset still requires the `PR body policy` check after the workflow reaches `main`.

Keep template requirements, validator behavior, tests, and documentation in the same independently deployable change so they cannot drift silently.
