# Buffering Lead State

- Task: authorized baseline commit, push and PR for cross-platform CI.
- Status: in_progress
- Inputs: [current revision/source](../progress.md), [canonical B0 contract and
  evidence](../baseline.md), current dirty/untracked B0.2-B0.6 source.
- Result: B0.2-B0.6 independently verified and actual-source accepted. Final host
  debug/release gates pass. Unchanged TCP retry passed; firewall cause unproven.
  Current timing capture JSON/hash/source identity verified and persisted.
  Rust 1.95 default/all-feature all-target builds and all-feature debug/release
  tests pass; default workspace tests including doctests now pass after approved
  optional-runtime README guards. Both enabled examples checked on both compilers.
- Authorization: baseline-only instrumentation, approved README guards and existing
  dependency policy. User approved committing baseline work, pushing this branch
  and opening a PR for CI. No product fixes/redesign or further installations.
  Preserve unrelated source; never access the main checkout.
- Blockers: host MSRV and doctest repair complete. No broader product fixes authorized.
  Non-host/baseline CI and independent released-peer resources remain unestablished.
  Exact full-memory/copy and recorder-overhead gaps are explicit, not passing.
  Zero-verifier READDIR defect remains unrepaired; architecture/numeric gates open.
- Next: inspect scope, commit current baseline changes with required coauthor,
  push current branch and open PR; inspect cross-platform CI results.
  Independent NFS peer also remains needed.
  Preserve measurement artifacts; obtain separate authorization for product work.
  Do not pause at routine
  stage boundaries. Only lead writes checkpoints.
