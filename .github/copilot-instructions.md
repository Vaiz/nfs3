# Repository Instructions

Rust NFS3 workspace: protocol types/codecs in `crates/nfs3_types`, derives in
`crates/nfs3_macros`, async client/server in their crates, integration/performance
tests in `crates/nfs3_tests`. Follow existing patterns and document public APIs.
Preserve unrelated worktree changes; keep edits scoped. Prefer the simplest change;
do not preserve incidental behavior (e.g. example output) unless asked.
Create extra worktrees under `.worktrees/` (git-ignored), not sibling directories,
so tool access stays inside the trusted repository and avoids approval prompts.

## Tool Use

Use directly callable tools whose schemas are already exposed. When the runtime marks
a required tool as deferred, use its provided schema/resource loader first, then call
the exposed tool; do not guess its arguments or add a repository-specific loader.
Report a blocker only when the runtime provides no permitted way to expose or invoke
the required tool.

## Rust Validation

Use Rust MCP for Cargo operations, not shell substitutes. Discover actual schemas;
missing tools/checks are blockers, never passes. Prefer workspace dependencies.
Read `.github/rust-toolchain.env` and CI for toolchains/targets; format with nightly.

For code changes:
1. Before editing: cargo-check with all targets/features.
2. After each substantive edit: the focused test/check that can falsify the change.
3. Completion: Clippy workspace/all targets/all features, warnings as errors;
   cargo-fmt all; tests all features; build all targets/features; machete; deny.

Maintain test coverage. Read-only verification uses fmt check and no auto-fixes.
Keep example regression tests in the crate's `tests/` directory, not in example
sources or example test targets.
Report unsupported targets and policy gaps explicitly; do not invent compliance
policy or install toolchains without authorization.

## Documentation and State

Keep one canonical statement per decision; link instead of duplicating. Keep docs,
instructions and handoffs short, actionable and current. Replace stale content.

Do not collect or retain historical data without a concrete current purpose.
Prohibited: session diaries, transcripts, repeated tool-failure narratives,
superseded proposals, resolved review rounds and redundant status snapshots.
Keep only current contracts, authorization, unresolved findings, next actions and
evidence needed for an active decision, reproducible baseline or safe resume.
Record exact revisions/commands/raw samples only where that evidence needs them;
do not accumulate them as a general archive. Prune when the purpose ends.
Preserve useful release/compatibility information, licenses and user-facing usage.
