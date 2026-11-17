# Buffering Docs State

- Task: DENY-PR verify five reported advisories and minimal patched versions.
- Status: completed
- Inputs: [B0 contract](../baseline.md), [pinned allocator evidence](../research.md#rallocator);
  Oxidizer `6021c37b6b8e5459d3e487d75d1fb2d5834fc02b`, rallocator 0.4.0,
  seismograph 0.5.0. Source worktree is interrupted/unverified.
- Scope: read-only official RustSec advisory pages/database and upstream release
  source/manifests for the five IDs in baseline.md. Establish real affected/patched
  ranges, MSRV 1.95 compatibility and whether chrono has a patched route. No
  edits, registry-only lookup, invented APIs/advisories or broad upgrade inventory.
- Result: all five official IDs and fix ranges verified; [pinned remediation
  evidence](../research.md#approved-advisory-remediation-evidence) persisted.
  Chrono has no patch; Jiff 0.2.28 std-only supports narrow example replacement.
- Blockers: none for the research scope; implementation/output-equivalence and
  lockfile/platform checks remain required.
- Next: Implement uses Tokio 1.53.3 and direct Jiff Timestamp conversion/display.
  User no longer requires Chrono formatting compatibility; no further research.