# Buffering Redesign Brief

- Redesign NFS3 buffering and deserialization to reduce payload copies and
  peak/live/retained memory without huge eager allocations from advertised lengths.
- Public API compatibility is not a goal. Valid RPC/XDR/NFS3 wire interoperability is.
- Shared buffers (`bytesbuf`, `bytes`), borrowing, and segmented storage are
  candidates, not mandated solutions. Account for retained backing allocations;
  a selective copy can be preferable to nominal zero-copy behavior.
- Draw on established crate implementations, not just novel abstractions.
- Distinguish source evidence, measurements, and hypotheses. Missing evidence is
  an explicit gap, not a passing result.
- Route open external design/API research through **Buffering Docs**; it uses
  authoritative docs and revision-pinned upstream source/tests, not just registry
  searches. Versions, licenses, advisories and MSRV come from local tools
  (`cargo-deny`, `cargo-info`, `cargo-tree`, `Cargo.lock`) without Docs. Reuse
  valid existing evidence; report inaccessible sources as gaps. Local repository
  inspection is each worker's responsibility.

## Resume State

Follow `docs/buffering-redesign/state/README.md`. Read only your supplied checkpoint
and necessary linked artifacts; verify inputs before reusing results. Return a
concise `Resume State` with result, evidence, blockers, and next action. Only the
lead writes checkpoints. Do not collect historical data without a current decision,
reproducibility, or safe-resume need; replace stale state instead of appending logs.