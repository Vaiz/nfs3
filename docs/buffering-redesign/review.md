# Open Review Findings

Last independent architecture verdict: REVISE after two rounds. No third unresolved
design loop. User approved correction directions below; D3.3 documents them but
does not constitute independent acceptance or proof. Behavior remains unauthorized.
Resolved findings and obsolete proposals are not retained.

| ID | Remaining proof/check |
| --- | --- |
| B1, high | Incremental receive can deadlock: two 8 MiB messages can each hold 5 MiB of a 10 MiB R share. Prove completion-safe admission before every grant, including read-ahead/future fragments. Test N records >R/N without eager allocation. Numbers are counterexamples, not defaults. |
| B2, high | Owned Payload + work token must move through blocking/owned VFS WRITE without forced copy; retain accounting until actual work ends. |
| B3, medium-high | Accounted writable READ buffer must allocate before backend work and expose only initialized prefix. Prove short-read clamping/EOF/error behavior; external Vec fallback cannot claim pre-allocation bounds. |
| B4, medium | IP:port/XID is not reconnect deduplication. Prove admitted mutations survive disconnect with ownership/token, and define conflicting XID, uncertain duplicate and outcome-slot exhaustion. Review digest collision/cost and cache/eviction policy. |

Clarifications retained in design: final-drop callbacks outside reentrant domain
locks and correct shutdown; pre-IO client guarantees only with finite reply bounds;
explicit Rust 1.95/rallocator placement in B0. These still need implementation proof.

Baseline-only work is independently authorized under [baseline](baseline.md):
hooks absent by default, measurement-only allocator, comparable compiler/allocator/
harness lanes, honest unsupported-case reporting. No architecture/performance pass;
bounded baseline code acceptance is recorded below. User agreement remains required for numeric resource/performance policy,
pool retention and transaction outcome/cache choices.

## B0.1 Patch Review

Review accepted the rebased B0.1 code at
`ecd86a186c990970d890463770a6c057e5979003`: actual codec hooks, feature isolation,
counter semantics, golden-wire tests and unsafe allocator fixture have no code
blockers. Review read actual files and relied on supplied diff/status evidence.
Full stage acceptance remains blocked by repository/measurement gates, separately
from architecture findings above.

- Prior B0-G3 dependency/policy finding is resolved: approved policy and prerequisite
  fixes are adopted, with combined-input deny passing. Broad gates are Implement's
  results, not independent Verify reruns.
- Local feature-off execution and capture build provenance are now established by
  lead follow-through after Review; [current evidence](baseline.md#b01-active-verification-evidence).
- Open G1/G2: CI has no separate feature-off lane; MSRV default-feature build omits
  allocator dependencies. New allocator code on non-host targets is unvalidated.
- G4 resolved: merged PR #174 uses isolated ports/temp storage and owned-child
  cleanup; current debug/release mirror execution and cleanup passed.
- Open G5: uncovered workloads, exact process-wide peak/live/retention/realloc
  metrics, allocator-recorder overhead, MSRV/non-host execution and independent
  peer interoperability remain missing. Host release and bounded lane timing passed.

Next: add dedicated baseline CI coverage before claiming the repository gate.
Measurement-only work may continue under B0;
whether exact end-to-end retention needs product redesign is unresolved.

Optional, not acceptance blockers: assert the observed last one-byte allocation
pair in the fixture; investigate thread-local initialization/destructor portability.
Before publication, confirm the intended public opt-in diagnostics feature surface.
No performance or exact memory acceptance follows from this patch review.

## B0.2 Patch Review

Scoped ACCEPT for the current executable and its untracked integration test;
[exact inputs and capture](baseline.md#b02-independent-capture). Review inspected
both actual files, relying on Verify for hashes/diff metadata and execution.
All [contract conditions](baseline.md#b02-scoped-opaque-retention) met; no blockers.
Eight validated events establish scoped requested-byte live/peak/retention and
final drops, not process-wide memory or performance. No new unsafe/product code.

Optional: pin the default lane's six-event output line in its integration test and
add a fifth-simultaneously-live synthetic allocation case. Strict eight-event
validation deliberately fails on recorder/toolchain allocation changes.
The untracked test must accompany the executable in any future commit.

Subsequent B0.3-B0.6 diagnostic slices have independent actual-source acceptance;
their current contracts, evidence and limitations are canonical in
[baseline](baseline.md#b03-independent-rpc-capture). Full B0 acceptance remains
subject to open gates; bounded feature-lane timing is not isolated hook overhead.
