# Buffering Status

- Scope: B0 baseline-only instrumentation with rallocator/Rust 1.95 and B1-B4
  document corrections authorized. No buffering behavior/prototypes or numeric policy.
- Design: [D3.3](design.md), condensed; not independently accepted.
- Research: [pinned evidence](research.md) available; no measured NFS results.
- Review: [B1-B4](review.md) remain proof/acceptance obligations after two rounds.
- Baseline: [B0](baseline.md) blocked by worker deferred-loader mismatch; no source
  patch, capture or Rust gate result. No active delegation.
- Current maintenance: docs/instructions/agent bodies condensed; duplicate design
  removed; current-only retention policy added. No Rust/manifests or model/tool
  selectors changed. Editor diagnostics clean; command-based validation unavailable.
- Next: resolve tool-loading prerequisite, refresh worktree and run B0's focused
  instrumentation slice, then independent capture/review. Do not retry unchanged
  tooling or treat documentation cleanup as implementation authorization.

Keep this as a current status summary, not a stage or session history.