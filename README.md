# buttter

`buttter` is intended to become a high-quality infinite-canvas diagramming application written in Rust using GPUI.

## Current Status

Phase 0.5 repository setup is complete. The repository contains no Rust application code or runnable application. The standalone Rust crate under `tools/repo-policy` enforces repository workflow rules; it is not application code.

No application investigation phase is authorized. Its scope and acceptance criteria must be approved before work begins. The repository policy tooling does not authorize GPUI work or application product code.

Repository workflow and review guardrails are defined in [AGENTS.md](AGENTS.md).

## Pull Request Policy

Pull request bodies must follow [the repository template](.github/pull_request_template.md). Validate a body locally with:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- validate-pr-body path/to/body.md
```

CI additionally verifies that the related `Closes #N` reference identifies an open, structurally complete work item. The work item must have a current `implementation-approved` label applied by `devdesignersid` before the pull request was created. The `PR body policy` check is required on `main`, so a ready-for-review pull request cannot merge when validation fails.

Commit messages must follow the repository's documented Conventional Commits grammar. The repository-managed `commit-msg` hook validates local commits. Validate a message directly with:

```sh
cargo run --manifest-path tools/repo-policy/Cargo.toml -- validate-commit-message path/to/message
```

Trusted CI requires exactly one commit in the GitHub pull-request commit list and validates its message. Git-generated merge commit messages and messages attributed by GitHub to a `[bot]` account are exempt from message validation, but not from the one-commit limit. The `Commit message policy` check must be required on `main`.

To update a pull request without adding another commit, stage the changes and amend its existing commit, then update the remote branch safely:

```sh
git add <paths>
git commit --amend
git push --force-with-lease
```

Do not bypass the repository-managed hooks. `--force-with-lease` refuses to overwrite remote work that was not present in the expected remote branch state.

See the [Repository Policy CLI documentation](tools/repo-policy/README.md) for the complete validation contracts, usage, CI trust boundaries, and maintenance workflow.

## Confirmed Intent

- macOS is the initial platform.
- Support for other platforms is a future goal, not a current claim.

## Project Information

- [Product definition](docs/PRODUCT.md)
- [Agent instructions](AGENTS.md)
- [Active work](https://github.com/devdesignersid/buttter/issues)
