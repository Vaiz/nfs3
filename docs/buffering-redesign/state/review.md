# Buffering Review State

- Updated: 2026-11-17
- Task: D3-RV2 independent design review, round 2 (final unresolved round)
- Status: completed
- Inputs: design-v3.md D3.2; research.md D3-R4/R6 and prior pinned comparisons; validation-plan.md D3-V3/V4; review.md R1-R11; user C2. Source HEAD 6b2d3aba001c78504f4029a85c199d6e3d920161 from Verify, no source changes; unrelated agent modification preserved.

## Completed
Round 1 complete: REVISE design D2. Findings R1-R11 saved in `../review.md`.
Research requirement satisfied except std Arc comparison. Rallocator isolation
accepted conceptually. No execution or source edits.
Round 2 D3-RV2 complete: architecture REVISE, behavior BLOCKED, baseline-only
authorizable under C1-C4 but not authorized. Findings persisted in review.md.
R1/R4/R5/R6/R7/R8/R9/R10 resolved in design; R2/R3/R11 partial.

## Evidence
`../design.md`, `../research.md`, `../validation-plan.md`. Pinned primary-source
mechanisms/tests and research summaries supplied for reviewer without web access.

## Open Issues
B1 receive hold-and-wait; B2 borrowed WRITE vs owned blocking work; B3 READ
preallocation/read-into and short replies; B4 IP:port identity, mutation completion,
duplicate dispositions/digest cost. N1 drop-under-lock/shutdown; N2 client max
reply; N3 specific rallocator/MSRV placement. User already permits MSRV lift.
Superseding user C2 withdraws D2 numeric gates entirely: none set. Pools/MSRV lift
allowed; unsafe init only sound+worthwhile; contiguous VFS preservation not goal;
bytesbuf user recommendation, not bytes; performance/low-friction defaults with
conservative configuration; ask before restrictions. D3-V4 passed measurement-plan
feasibility for second review, not code/policy approval. D3.2 carries clarifications.

## Next Action
Lead escalates B1-B4/N1-N3 and proposed fixes; no third review loop. Baseline-only
may be authorized separately. No code/performance approval or new policy imposed.