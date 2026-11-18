# Buffering Verify State

- Task: host MSRV validation with user-installed Rust 1.95.
- Status: completed
- Inputs: HEAD d1f731b07c7e7b664b3a6108231d3ce3d10a92a3 plus dirty B0.2-B0.6;
  canonical [accepted baseline evidence](../baseline.md), current manifests/CI.
- Scope: read-only source, Rust MCP locked default/all-feature workspace
  all-target builds and tests on 1.95, including allocator/RPC diagnostics.
  Separate target/msrv-1.95 directory; preserve all measurement binaries/artifacts.
- Result: default/all-feature all-target builds and all-feature debug/release
  workspace tests PASS. Default executable tests PASS; default doctests FAIL
  on pre-existing unguarded Smol README example. Later doctests not reached.
  Source identity unchanged; [exact current evidence](../baseline.md#host-msrv-validation).
- Blockers: none for host MSRV after authorized maintenance follow-up; Implement
  fixed optional-runtime README guards and passed the full default workspace gate.
  Independent initial source validation remains distinct from those repair checks.
- Next: external platform/peer gates remain; current combined evidence is canonical
  in baseline.md, not a claim that the initial failing run itself passed.
