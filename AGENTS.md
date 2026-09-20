# Agent Instructions

## Scope

These instructions apply to the entire repository.

## Workflow

- Work on one explicitly approved behavior at a time.
- Obtain acceptance criteria before implementation.
- Ask about missing product requirements; do not infer them.
- Before making implementation changes:
  - state the problem being solved;
  - describe the proposed solution;
  - explain why it is the best fit for the approved requirements and current constraints;
  - explain how it avoids unnecessary complexity and overengineering.
- Wait for the user to approve the proposed approach before implementing it.
- Keep every change small enough for complete human review.
- Show the complete diff and wait for the user to say "Approved" before committing.
- Before committing, inspect the proposed commit contents and exclude every file, including dotfiles, that is outside the approved change scope.
- Format every commit message according to Conventional Commits 1.0.0.
- Never commit without that approval.
- Claim completion only after all acceptance criteria and applicable quality gates pass.

## Evidence

- Separate verified facts, hypotheses, recommendations, decisions needed, commands executed, files changed, and unverified items in every report.
- Support repository facts with file-and-line references or command output.
- Support external facts with a primary source, version or revision, and access date.
- Support performance claims with reproducible measurements.
- When technical behavior is uncertain, propose a minimal proof of concept.

## Testing

- Before writing product code, write automated tests covering every approved acceptance criterion and all identified edge cases, boundary conditions, and error cases.
- Explain how the proposed test cases map to the approved requirements before writing product code.
- Run the new tests and confirm they fail for the expected reason before implementing the behavior.
- Write only the minimum product code required to make all tests pass.
- Require 100% line coverage for repository-authored product code. Test code, generated code, and third-party source are excluded from this requirement.
- Do not claim completion unless all tests and the applicable coverage gate pass.

## Implementation

- Prefer executable quality gates over prose standards.
- Before using a third-party API, review the documented failure modes for the exact dependency version.
- Handle, propagate, or explicitly rule out each documented failure mode according to the approved behavior; never silently discard an error.
- Do not treat changes to third-party source as project scope unless the user explicitly approves them.
- During the proposed solution, identify the state transitions and failure paths that require diagnostic logs.
- Add only the approved diagnostic logs; do not add speculative logging.
- Write code comments, commit messages, documentation, and other developer-facing text in clear, direct language that communicates intent without unnecessary jargon.
- Use the least complex solution that satisfies the approved acceptance criteria.
- Do not add speculative abstractions, dependencies, portability layers, algorithms, or features.
- Keep domain logic independent of GPUI where practical, but introduce an abstraction only when a concrete requirement justifies it.

## Pre-Completion Code Review

After implementation and before claiming completion, review the complete diff and report evidence for every applicable item:

- Requirement fit: restate the original task and verify that the implementation satisfies each approved requirement.
- Behavior boundaries and regressions: verify when new logic applies and that all previously documented requirements remain satisfied.
- Design: verify that each abstraction is justified, each new function has a clear purpose, and modified functions have not accumulated unrelated responsibilities.
- Tests: verify that business logic is covered by tests required by the Testing section.
- Correctness and readability: inspect for known errors, invalid logic, unclear naming, and unnecessarily difficult control flow.
- Standards: verify compliance with every applicable repository standard, using executable quality gates where available.
- Scope: compare the diff with the approved scope and identify any scope creep.
- Maintainability: identify concrete coupling or constraints introduced by the change that would obstruct known extension requirements.
- Side effects: identify state changes and their affected callers or dependents, then check for unintended behavior, single points of failure, and increased blast radius.
- Documentation: determine whether the change affects behavior, interfaces, operations, or developer workflows that require documentation updates.
- Evidence: support each conclusion with file-and-line references, test or quality-gate output, command output, or an applicable primary external source.
- Unverified items: do not assume a check passed. Mark anything that cannot be verified as unverified and request a decision when needed.

## Branching

- Use Continuous Integration with `main` as the mainline.
- Never commit or push changes directly to `main`.
- Do not make working-tree changes while `main` is checked out.
- Create a short-lived branch before making each change.
- Keep the committed state of `main` healthy.
- Integrate changes into `main` only through a pull request whose applicable tests and quality gates pass.
- Integrate each approved, healthy increment as soon as it can be shared.
- Do not accumulate more than one day of implementation work without opening or updating its pull request.
- Do not leave completed work on a long-lived branch.

## Human Review Scope

- The user reads every line of repository-authored files.
- Generated files, including `Cargo.lock`, do not require line-by-line user review.
- Vendored and downloaded third-party dependency source does not require line-by-line user review by default.
- Identify generated, vendored, and downloaded third-party files in change reports.
- Flag any concrete concern discovered in excluded third-party source and ask whether the affected material requires user review.

## Pre-Implementation Restrictions

Until the user explicitly authorizes implementation:

- do not create Rust code;
- do not install tools or dependencies;
- do not modify files other than an explicitly approved `AGENTS.md`;
- do not commit.
