# Buffering Status

- Scope: B0 baseline-only instrumentation with rallocator/Rust 1.95 and B1-B4
  document corrections authorized. No buffering behavior/prototypes or numeric policy.
- Prerequisites: [PR #171](https://github.com/Vaiz/nfs3/pull/171),
  [PR #173](https://github.com/Vaiz/nfs3/pull/173) and safe mirror harness
  [PR #174](https://github.com/Vaiz/nfs3/pull/174) are merged. Current base is
  `origin/main` `00ea3aa2f2d4027bb9ead8da2ee71d647e88f40e`.
- Active source: current session branch `vaiz-redesigned-dollop`, rebased cleanly
  onto that main revision at `d1f731b07c7e7b664b3a6108231d3ce3d10a92a3`.
  The user approved proceeding without branch rename. No push or merge performed.
  Historical external worktrees are not inputs to modify or access.
- Completed slice: B0.2 scoped opaque retention; [independent capture](baseline.md#b02-independent-capture)
  and actual-patch review accepted the bounded result.
  Source changes are limited to the measurement executable and its new untracked
  integration test. [Current evidence](baseline.md#b02-independent-capture) records
  1,380,352 requested-byte peak, four final drops and zero final live output bytes.
  Host gates passed. Safe mirror execution passed 48 cases per debug/release profile,
  with owned-child/temp cleanup confirmed.
- Completed: B0.3 end-to-end metadata/READ/WRITE ledger, independently captured
  and actual-patch accepted; [current evidence](baseline.md#b03-independent-rpc-capture).
- Completed: B0.4 event-visible cohort independently reconstructed and actual-patch
  accepted; [current measurements](baseline.md#b04-independent-allocation-capture).
- Completed: B0.5 bounded framing/directory/slow-peer/concurrency/loopback coverage,
  independently verified and actual-source accepted; [evidence](baseline.md#b05-independent-wire-capture).
- Completed: B0.6 normal-allocator timing and whole-feature-lane comparisons,
  independently captured and actual-source accepted;
  [measurements and limitations](baseline.md#b06-independent-timing-capture).
  Full B0 is not complete; no process-wide memory or performance claim.
- Design: [D3.3](design.md), not independently accepted. Existing
  [research](research.md) remains available; [review obligations](review.md) remain open.
- Separate harness prerequisite landed; dedicated baseline feature-off and allocator
  MSRV CI coverage remain absent. The merged workflow fixes debug/release harness
  binary selection, not these baseline-specific lanes.
  Remaining: uncovered workloads, exact process-wide/copy metrics, recorder
  overhead, non-host execution and independent peer interoperability.
  Host Rust 1.97 debug/release gates pass. Rust 1.95 default/all-feature builds,
  default tests and all-feature debug/release tests pass. Authorized optional-runtime
  doctest guards are fixed and checked in default/Tokio/Smol configurations on
  both compilers. [MSRV evidence](baseline.md#host-msrv-validation).
  User authorized committing/pushing baseline work and opening a PR for
  cross-platform CI. Product fixes/redesign remain unauthorized.

Keep this as a current status summary, not a stage or session history.
