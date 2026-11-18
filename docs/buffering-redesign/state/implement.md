# Buffering Implement State

- Task: user-authorized client README doctest feature guard.
- Status: completed
- Inputs: current dirty B0.2-B0.6 workspace, explicit approval for runtime
  example feature guards; [canonical evidence](../baseline.md#host-msrv-validation).
- Scope: client README only; existing rpc-diagnostics bullet preserved.
- Result: added 12 hidden feature-guard/fallback-main lines; actual enabled
  examples remain compiled no_run tests. Default/Tokio-only/Smol-only doctests
  pass on Rust 1.95 and 1.97.0, two passed and zero ignored each.
  Rust 1.95 default workspace tests including doctests pass. Rust 1.97
  workspace all-target/all-feature precheck and diff whitespace check pass.
- Blockers: none for this maintenance fix. No product/dependency/CI changes,
  ignored examples, installs, commits or publication.
- Next: external platform/peer resources and broader baseline gates remain
  separate; no repeated maintenance review or unchanged broad checks needed.
