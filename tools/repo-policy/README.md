# Repository Policy CLI

`repo-policy` enforces the objective pull request body, commit-cardinality, and commit-message requirements defined by this repository. It gives contributors immediate local diagnostics and gives CI a deterministic pass-or-fail result.

The tool does not judge subjective requirements such as readability, design quality, appropriate scope, or whether evidence is truthful. Those requirements still need human review.

The pinned `sha2` 0.10.9 crate supplies the SHA-256 implementation. Its fixed-input `Digest::update` and `Digest::finalize` API is infallible; input and GitHub failures are handled before hashing. No optional assembly feature is enabled. The pinned `toml` 1.1.6 parser returns structured errors for malformed input, and the pinned `serde` 1.0.229 derive implementation maps valid TOML into a schema that rejects unknown fields. Manifest read, parse, schema, path, ownership, process, and report failures are propagated as policy failures.

## Prerequisites

- Rust and Cargo with Rust 2024 edition support. CI uses Rust 1.98.1.
- `cargo-llvm-cov` 0.9.1 when running repository quality targets.
- [GitHub CLI](https://cli.github.com/) authenticated for the target repository when using `--github-repository`. Structural validation does not require GitHub CLI or network access.

The Rust crate makes no third-party network requests at runtime.

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

Omit the path to read a message from standard input. Require exactly one commit in a pull request and validate its message through the authenticated GitHub CLI:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-commits \
    --github-repository devdesignersid/buttter \
    --pull-request-number 10
```

Validate the pull request's complete approved file scope:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-pr-file-scope \
    --github-repository devdesignersid/buttter \
    --pull-request-number 10
```

Validate the repository quality manifest and print its CI matrix:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-quality-manifest .github/quality-targets.toml
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  quality-matrix .github/quality-targets.toml
```

Run every gate for one registered target:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- \
  run-quality-target .github/quality-targets.toml repo-policy
```

Print built-in help:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- --help
```

A successful invocation identifies the policy that passed and exits with status 0. Invalid arguments, unreadable or non-UTF-8 input, policy violations, GitHub CLI failures, API failures, and malformed GitHub data produce diagnostics on standard error and exit with status 1.

## Approved-file-scope contract

The work item's `Review scope` section must contain exactly one declaration in this form:

````md
<!-- approved-paths:start -->
```json
[
  ".github/workflows/example.yml",
  "Cargo.lock",
  "src/example.rs"
]
```
<!-- approved-paths:end -->
````

The JSON array must contain one or more unique, exact repository-relative UTF-8 file paths in ascending byte order. Paths cannot be empty or absolute, end in `/`, contain NUL, use empty, `.` or `..` components, or contain the glob metacharacters `*`, `?`, `[` or `]`. Directory and glob patterns are not supported. Dotfiles and generated files receive no implicit coverage and must be listed explicitly.

The canonical digest input preserves declaration order. For each decoded path, append its UTF-8 byte length in decimal, one colon, and its UTF-8 bytes, with no separator between entries. SHA-256 of that byte sequence is the scope digest. This script prints the digest for an issue body saved as `issue.md`:

```sh
python3 - <<'PY'
import hashlib
import json
from pathlib import Path

body = Path("issue.md").read_text()
start = body.index("<!-- approved-paths:start -->")
end = body.index("<!-- approved-paths:end -->")
block = body[start + len("<!-- approved-paths:start -->"):end].strip()
paths = json.loads(block.removeprefix("```json\n").removesuffix("\n```"))
digest = hashlib.sha256()
for path in paths:
    encoded = path.encode("utf-8")
    digest.update(str(len(encoded)).encode("ascii") + b":" + encoded)
print(digest.hexdigest())
PY
```

Before creating the pull request, `devdesignersid` must post an issue comment containing exactly:

```text
scope-approved sha256:<64-lowercase-hex-digest>
```

The latest surviving comment whose body starts with `scope-approved` controls. Its author must be authorized; its creation and latest-update timestamps must both strictly predate pull-request creation; and its digest must match the current declaration. Missing, malformed, deleted, unauthorized, stale, edited after pull-request creation, or post-PR approval records fail closed.

`validate-pr-file-scope` obtains the linked issue from the validated pull request body. It verifies that the issue is open and structurally complete, then reads the declaration, approval comments, pull-request metadata, and paginated pull-request file list through GitHub API version `2022-11-28`. Additions, modifications, deletions, and the `changed` and `unchanged` statuses require the current path to be approved. Renames and copies require both current and previous paths. An empty diff passes when the declaration and approval are valid. Duplicate or malformed file records, changed-file count mismatches, more than the endpoint's 3,000-file limit, invalid encoding, unknown statuses, pagination failures, authentication failures, API failures, and GitHub CLI failures fail closed.

Scope expansion requires a new work-item declaration and a new digest-bound approval, followed by a new pull request. Do not update or reuse the old pull request. Because GitHub does not emit a pull-request event for every linked-issue state change, rerun the check before relying on an earlier result if issue data may have changed.

## Repository-quality contract

`.github/quality-targets.toml` is the machine-readable inventory of repository-authored code targets. Each target declares one Rust Cargo manifest, its platform, product-source roots, test roots, generated paths, third-party paths, and supporting files. Lists are exact, sorted repository-relative paths. Categories cannot overlap, and target ownership cannot overlap. Rust source files and executable tracked files must belong to a target. Supporting shell hooks are owned and tested by `repo-policy`, but only Rust files under `product_sources` are included in the line-coverage denominator.

Only `rust` targets are currently permitted. `linux` maps to `ubuntu-24.04`; `macos` maps to `macos-15`. Adding another target or platform requires a manifest and policy change rather than an unregistered workflow command.

`run-quality-target` verifies the pinned tool versions, then runs Rustfmt, Clippy with warnings denied, and `cargo llvm-cov`. The coverage command executes the target's tests while producing LCOV, so tests are not run twice. LCOV must contain every Rust product file, no non-product file, valid line records, and no line with a zero execution count. Missing tools, nonzero subprocess results, missing files, malformed reports, and omitted files fail closed. Tests, generated paths, third-party paths, and supporting files are outside the product-code line denominator.

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

`validate-pr-commits` defines cardinality from the paginated GitHub pull-request commit list and requires exactly one unique commit. Pull-request metadata is used only to detect an incomplete or inconsistent paginated list. GitHub API version `2022-11-28` limits this endpoint to 250 commits, so larger pull requests fail closed with a pagination-limit diagnostic. Zero commits, multiple commits, pagination mismatches, missing or duplicate commits, malformed records, invalid encoding, and API errors fail closed explicitly.

After cardinality passes, the command validates the one commit's message. Its message is exempt when GitHub attributes the commit to a non-empty login ending in `[bot]`; the commit itself is not exempt from the cardinality limit. Names or email addresses embedded in Git commit metadata do not establish the message exemption. Nonconforming non-bot messages fail closed with a diagnostic identifying the rejected commit SHA.

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

`.github/workflows/commit-message-policy.yml` runs for pull request creation, edits, reopening, commit synchronization, and transitions to ready for review. Draft pull requests may remain failing while incomplete; ready-for-review pull requests must contain exactly one commit and pass message validation. The `edited` event covers base-branch changes, while `synchronize` covers commits being added, removed, or rebased.

The workflow uses `pull_request_target` and trusted policy code from the pull request's base commit. It reads the paginated commit list and pull-request metadata through the GitHub API and never checks out or executes pull-request code. Its token has only `contents: read` and `pull-requests: read` permissions.

`.github/workflows/file-scope-policy.yml` runs for pull request creation, edits, reopening, commit synchronization, and transitions to ready for review. It checks out and executes only policy code from the pull request's base commit. It treats pull-request and issue content as data, never checks out or executes pull-request code, and has only `contents: read`, `issues: read`, and `pull-requests: read` permissions.

`.github/workflows/repo-policy-quality.yml` runs on every pull request and every push to `main`, so an unregistered source path cannot bypass the workflow's path filters. It validates the manifest, derives a Linux/macOS matrix, and runs every registered target from pull-request code with read-only repository permissions and no secrets. The final `Repository quality` job aggregates planning and matrix results into one stable required-check context.

Policy jobs have explicit five-minute timeouts. Repository-quality planning and aggregation have ten- and five-minute timeouts, and target jobs have a 30-minute timeout.

### Server-side enforcement

The `PR body policy`, `Commit message policy`, `File scope policy`, and stable `Repository quality` status checks are required on `main`; otherwise CI would only detect and report violations.

Online issue verification reflects the issue state when the workflow runs. If an issue is closed without another supported pull request event, rerun the workflow before relying on the previous result.

## Amending a pull request commit

Keep a pull request at one commit by amending instead of creating a follow-up commit:

```sh
git add <paths>
git commit --amend
git push --force-with-lease
```

Do not use `--no-verify` or otherwise bypass repository-managed hooks. Review the amended commit before pushing. `--force-with-lease` fails rather than overwriting remote work when the remote branch no longer matches the state known locally.

## Development

Install the exact CI tools and reproduce the manifest-driven gate from the repository root:

```sh
rustup toolchain install 1.98.1 --component rustfmt --component clippy
cargo install cargo-llvm-cov --version 0.9.1 --locked
cargo +1.98.1 run --manifest-path tools/repo-policy/Cargo.toml -- \
  validate-quality-manifest .github/quality-targets.toml
cargo +1.98.1 run --manifest-path tools/repo-policy/Cargo.toml -- \
  run-quality-target .github/quality-targets.toml repo-policy
```

The individual Rust gates remain useful while iterating:

```sh
cargo fmt --manifest-path tools/repo-policy/Cargo.toml -- --check
cargo clippy --manifest-path tools/repo-policy/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path tools/repo-policy/Cargo.toml
cargo llvm-cov --manifest-path tools/repo-policy/Cargo.toml --fail-under-lines 100
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
