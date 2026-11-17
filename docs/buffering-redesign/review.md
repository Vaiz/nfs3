# Independent Design Review

## D3-RV2, Round 2: REVISE (Final Design Round)

2026-11-17. Review inspected design-v3.md D3.2, research D3-R4/R6 and prior
comparisons, validation-plan D3-V3/V4 and actual source. No execution, Git check
or edits by reviewer. Architecture REVISE; behavioral implementation BLOCKED.

### Technical Blockers

| ID | Severity | Failure and recommended resolution |
| --- | --- | --- |
| B1 / R2 | High | Incremental receive credit can deadlock: with R=10 MiB, two valid 8 MiB messages can each hold 5 MiB and wait forever. E progress does not release incomplete R records. Specify a non-circular receive grant rule, e.g. completion capacity for admitted records with a large-record grant, or sufficient whole-fragment reservation at each marker. Reject cumulative lengths that can never fit attainable credit. Test N simultaneous records larger than R/N. Values are counterexamples, not defaults. |
| B2 / R3 | High | Borrowed PayloadRef across VFS write cannot move into tokio::fs/spawn_blocking/io_uring-style owned work without a copy. Propose owned Payload for server VFS write; borrowed convenience secondary. Work retains payload/accounting/token through actual completion. |
| B3 / R3 | Medium-High | Backend READ allocates before protocol adoption/check, so largest allocation is not prebounded and bulk READ reuse is limited. Propose accounted writable read-into buffer reserved from operation entitlement; external Vec adoption is an explicitly unbounded-backend fallback. Short READ is legal: clamp to affordable count instead of generic rejection. Define error when clamping cannot help. |
| B4 / R11 | Medium | Current client_addr is peer_addr().to_string(), i.e. IP:port, not IP alone; reconnect protection limited. Cancellation rules conflict with detached tokio::fs work. Propose letting admitted mutations finish with owned payload/token and discarding disconnected replies, or a concrete VFS work-token API. Define different-args XID reuse, already-written mutation duplicate, and outcome-slot exhaustion responses. Full-payload identity hashing has hot-path cost; compare identity options and expose collision tradeoffs for user decision. |

### Required Clarifications

- N1: final-drop callbacks must not run while holding a domain lock they reacquire.
	Trim/reserve detach under lock, release outside; specify release after shutdown.
- N2: pre-IO client budget checks require known/configured maximum replies; for
	wire-unbounded replies like READLINK, late exhaustion may close/poison after send.
- N3: make rallocator harness/compiler placement explicit in baseline scope.
	Reviewer wording that MSRV increase was not approved is corrected by C2: the
	user already allows increases. Specific Rust 1.95 wiring remains a stage choice,
	not a justification for a mandatory isolated harness.

### Status and User Gate

Resolved in design: R1, R4, R5, R6, R7, R8, R9, R10. Partial: R2 (B1), R3
(B2/B3), R11 (B4). Proof/compile/measurements remain future requirements, not results.
Resource grants/shares/limits/deadlines, pool retention, tracker outcomes/cache and
measured acceptance criteria require agreement. D2 numeric gates are withdrawn.
Reviewer mentioned prefix checksums as an identity option; lead has NOT adopted
this weaker identity tradeoff or asserted equivalence to full request identity.

Baseline-only work can be authorized independently because it changes no buffering
behavior. Reviewer conditions C1-C4: feature-gated hooks absent by default; explicit
rallocator/MSRV placement; same compiler/allocator/harness per comparison lane with
instrumented/normal runs; unsupported baseline workloads labeled unsupported.
Prototype/behavior changes remain separately gated. No source authorization yet.

Lead disposition: STOP after second unresolved round, no third design review loop.
Present B1-B4 and recommended fixes to user; preserve rejected proposal and policy
questions. Review text is not implementation authorization. No code/performance pass.

## D2-RV1, Round 1: REVISE

Buffering Review inspected design D2, research D1-R1/D1-R2, independent measurement
assessment and actual Rust paths. No builds, runs or edits. Findings below are
design blockers, not deferred execution results.

| ID | Severity | Finding and required resolution |
| --- | --- | --- |
| R1 | High | Whole 8 MiB credits retained by copied handles exhaust 32 MiB client domain after four replies; download_folder queue is a concrete failure. Charge detached values by capacity, release record credits, add tree traversal/retained-handle workload. |
| R2 | High | 32 header-only connections occupy every global record reservation for 30 s; metadata RPCs also waste 8 MiB each. Use phased/size-aware charges and per-peer assembly fairness, early-progress deadline; test 64/128 active and header-only attack. |
| R3 | High | RPC metadata displaces payload into multiple receive chunks, so 4 KiB+ WRITE normally coalesces. Server WRITE <=75% peak target is implausible with contiguous VFS. Evaluate segmented VFS and explicit fallback; any target relaxation requires user approval. |
| R4 | High | Default <=64 KiB sender fragments break released client, which rejects fragmented replies; streaming slices inside one fragment gives the same memory benefit. Default one fragment; add mixed-version tests. |
| R5 | High | List<Vec> and 1 MiB host expansion/64 KiB allocation cap conflict; READDIRPLUS host expansion exceeds wire size. Define paged List API, client request bounds, server host-reply ledger and incremental bounded response construction. |
| R6 | Medium | Marker/body/trailer tiny writes with client Nagle enabled can stall. Coalesce marker+metadata, address trailing padding via bounded copy/vectored IO/nodelay; test real Nagle-enabled transport. |
| R7 | Medium | Direct marker+body reads increase small-RPC syscalls. Specify bounded read-ahead and record read/write call counts. |
| R8 | Medium | Error mapping wrong/underspecified: bounded bad args/padding should GARBAGE_ARGS, credentials AUTH_BADCRED, WRITE count mismatch NFS3ERR_INVAL, overlong names NAMETOOLONG. RFC filename/path are unbounded strings; local policy must match PATHCONF. UnexpectedXid must poison. |
| R9 | Medium | Backing already needs Arc for ticket/identity; Bytes adds ownership/control cost without exposed value. Compare std Arc<Vec<u8>> alternative before dependency choice. |
| R10 | Medium | Separate OpaqueRef/owned Opaque does not specify generated struct lifetimes. Choose type family, keep metadata contiguous and payload segmented; cover derives, handle slicing, VFS names, examples. |
| R11 | Medium | Cancellation/unsent replies must not leave completed duplicate-suppression entries blocking retries. Specify transaction states/removal and side-effect/idempotency caveat. |

Optional/low: quantify >4 KiB view retention; scope idle timeout to in-progress
records/writes; document 30 s absolute timeout on slow links; safe Tokio read_buf
could avoid initialization but is not required.

Research requirement met (http_headers plus independently chosen established
crates, pins and reuse decisions); std ownership comparison remains missing.
Isolated rallocator harness accepted conceptually; every allocator lane must use
and record identical baseline/candidate toolchains. Baseline, threshold locking,
tracker exactness, overhead, independent peer and target gates remain properly
deferred. Add read/write call counts, Nagle, above-cap concurrency, header-only
starvation, large directories, retained handles and old-client workloads.

Direct source inspection included server tcp/rpcwire/messages, client rpc/io/tokio/
error, xdr opaque/list/primitives/util, macros struct derive, NFS/RPC types, VFS,
handlers, transaction tracker, download_folder and Cargo.lock. Source HEAD supplied
by Verify: 6b2d3aba001c78504f4029a85c199d6e3d920161. No new Git verification claimed.

Next: D3 resolves R1-R11, Verify reassesses changed limits/thresholds, then second
review. After two unresolved rounds, stop and present concrete remaining issues.