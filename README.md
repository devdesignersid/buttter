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

See the [Repository Policy CLI documentation](tools/repo-policy/README.md) for the complete validation contract, usage, CI trust boundary, and maintenance workflow.

## Confirmed Intent

- macOS is the initial platform.
- Support for other platforms is a future goal, not a current claim.

## Project Information

- [Product definition](docs/PRODUCT.md)
- [Agent instructions](AGENTS.md)
- [Active work](https://github.com/devdesignersid/buttter/issues)
