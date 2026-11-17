# Buffering Docs State

- Updated: 2026-11-17
- Task: D3-R6 finish exact custom block API evidence
- Status: completed
- Inputs: design.md User Directions C2; research.md D3-R4; review.md R1/R2/R9; same source snapshot, no new Git verification.

## Completed
Completed D1-R1 and D1-R2 research. rallocator resolves to Microsoft Oxidizer 0.4.0,
revision 6021c37b6b8e5459d3e487d75d1fb2d5834fc02b, MSRV 1.95/MIT. No published
version fits 1.88. Event counters are not exact capacity/retention/copy counters.
Both invocations returned successfully without model override; no source edits.
Completed D3-R4: current bytesbuf/bytesbuf_io 0.14.0, GlobalPool/plurality,
uninitialized IO and Arc comparison. Result persisted in research.md D3-R4 before
dependent D3-R5. Direct reuse plausible; custom provider API feasibility unresolved.
D3-R5 returned an incomplete report; persisted in research.md. No finished provider
proof or exact signatures supplied. This is not a successful feasibility check.
D3-R6 completed public custom-block feasibility; evidence and full canonical pin
persisted in research.md. Public unsafe extension permits metadata, final-drop
accounting/recycling and prepared blocks without GlobalPool; no full fallibility.

## Evidence
Durable evidence and pinned source/test/benchmark links: `../research.md` D1-R1/D1-R2.
No NFS measurements or cross-target builds were performed.

## Open Issues
D3-R3 was interrupted/unknown; no returned comparison is persisted. Superseded
by D3-R4, incorporating its std Arc comparison rather than assuming it completed.
GlobalPool lacks exact origin/capacity and trim hooks; Memory::reserve infallible.
Need concrete safe construction/public metadata/lifecycle APIs for preallocated,
charged custom blocks. No invented provider capabilities.
MSRV increases/pools allowed; unsafe initialization needs soundness and benefit
proof. User recommends bytesbuf, not bytes. No performance gates or numeric
limits approved; ask before imposing restrictions. No source edits authorized.

## Next Action
Lead incorporates D3-R4/R6 into design, with proof/measurement gates for custom
block adapter and optional unsafe initialization; no code or performance proven.