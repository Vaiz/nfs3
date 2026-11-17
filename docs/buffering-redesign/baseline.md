# B0 Baseline-Only Stage

## Authorization and Scope

2026-11-17: user answered "approved" to the lead's explicit request for the four
design corrections and baseline-only instrumentation using rallocator and Rust 1.95,
with buffering behavior unchanged and prototypes separately authorized. This is
not blanket authorization for redesign implementation, numeric limits or performance
gates. D3-RV2 permits baseline-only work independently of architecture B1-B4 under
its C1-C4 conditions. This document is the accepted, bounded B0 implementation scope.

Lead edits coordination/design documents only. Buffering Implement is the sole
Rust/test/benchmark writer. Verify independently captures/checks the baseline, and
Review inspects actual patch and evidence. No concurrent writers or benchmarks.
Preserve unrelated worktree changes, including the agent file reported by Verify.

## Permitted Changes

- Raise workspace rust-version to 1.95 and update directly affected MSRV documentation
  or checks. CI's existing pinned 1.97.0 compiler/nightly formatter need not change.
  Record actual available compilers; use identical compiler per comparison lane.
  Do not silently install/update toolchains or system tools if unavailable.
- Add pinned rallocator 0.4.0 and necessary matching event-recording dependencies
  at workspace level, used only by an opt-in measurement executable/bench in
  nfs3_tests. Verify actual API/version/license compatibility. No product global
  allocator replacement, no bytesbuf/provider prototype, no unrelated updates.
- Feature-gated measurement hooks at existing receive, growth, codec, send and
  backend/test transport copy sites where required. Default feature set compiles
  them out. Do not repair protocol behavior, change scheduling, alter allocation
  strategy, payload ownership, VFS interfaces, IO semantics or retry handling.
- Extend existing test/perf modules and benches where practical. Add a dedicated
  measurement executable only where allocator registration/machine-readable output
  requires it. Counters/labels are instrumentation, not a new buffer architecture.
- Tests for tracker self-consistency and exact wire-byte equivalence between hook
  configurations; controlled adversarial workloads report current limitations
  instead of fixing them. No deliberate OOM or unbounded hang.

No policy restrictions or numeric performance gates are adopted. Finite workload
sizes/timeouts are harness safety/reproducibility settings, not product defaults;
record them explicitly. Do not run advertised-length OOM probes against old code.

## Instrumentation Contract

Capture process allocation events via rallocator and report requested size/count,
largest allocation, free/realloc events, capture start state and lost-event count.
Validate recording recursion, address reuse, cross-thread attribution/order and
realloc semantics before reconstructing exact live counts. Use origin/site hooks
for protocol backing capacity, lifetime and copy/initialization bytes where allocator
events cannot establish them. Separate payload/metadata/control/backend/runtime/
test transport/collector costs; unknown attribution remains unknown, not zero.

Record explicit copy sites and zero-fill separately. Vec growth may move storage;
do not infer exact copied bytes from capacity change alone. If allocator semantics
prevent exact movement counts, report bounds/unknown rather than a fabricated
measurement. Retained output is tracked until actual drop; address alone is not
origin identity. Current baseline has no new protocol pool; do not implement pool
tests against nonexistent code and label absent features as absent/unsupported.

Machine-readable evidence includes Git revision plus dirty diff/untracked source,
toolchain/profile/target/features/allocator, workload/seed/runtime/backend, run
counts/timing boundaries, counters, raw durations and per-metric certainty. Use
stable units and keep instrumented diagnostic and normal-allocator timing lanes
distinct. Instrumentation overhead is measured with comparable paired workloads;
do not subtract estimated overhead or claim improvement from baseline alone.

## Workloads and Initial Slice

Start with one deterministic opaque encode/decode or existing RPC slice to prove
counter accuracy and unchanged bytes. Cheapest discriminating check: known payload
size, recorded copy/initialization counts and golden bytes must agree with the
unchanged implementation; feature-disabled behavior must be identical. Run that
check immediately after first substantive source edit before widening scope.

Then cover existing NULL/GETATTR/LOOKUP and READ/WRITE sizes 4/64/256/1024 KiB,
retained READ outputs, directory/retained-handle traversal and relevant current
fragmentation/short-read cases. Add bounded header-only/slow-peer/concurrency cases
where safely runnable without changing behavior. Current fragmented client receive
and some cancellation cases may be unsupported; label honestly. Candidate-only
bytebuf pools, read-into and unsafe initialization are NOT B0 work.

Verify must distinguish measured coverage, missing instrumentation and unsupported
baseline cases. Baseline cannot be declared complete while required copy/capacity/
lifetime metrics lack validated instrumentation. If initial scope cannot establish
them without behavior changes, stop with the exact gap; do not smuggle redesign
code into B0. No perf acceptance ratios are set.

## Execution and Acceptance

Use Rust MCP for Cargo per repository rules: check all_targets/all_features before
source edits; focused tests immediately after edits; strict workspace Clippy;
nightly formatting (Verify check-only); all-feature tests; all-target/all-feature
build; machete; deny. Use locked CI/MSRV and runtime matrix where available; record
missing tools/targets/policy as blockers, not passes. Toolchain installation or
shell Cargo fallback is not granted by this stage. If timing needs cargo bench
without an MCP equivalent, request the explicit exception before executing it.
Running an MCP-built measurement executable for this authorized capture is within
the agreed measurement work; record its exact command and settings.

Acceptance for B0: unchanged default behavior/wire bytes, opt-in hooks and allocator,
validated counters with honest domains, reproducible independent baseline evidence,
no unresolved relevant code-review findings, and repository gates or explicitly
blocked requirements. No speedup expected/claimed. Implementation self-tests do
not substitute for Verify capture or Review. No commits/branches unless requested.

## Evidence Status

Initial source snapshot: 6b2d3aba001c78504f4029a85c199d6e3d920161. Worktree must be
inspected again before editing. No B0 source patch or measurement exists yet.
Implement checkpoint: state/implement.md. Subsequent outcomes are appended here
by lead, with Verify/Review inputs bound to exact patch state.

## B0-I1 Attempt: Tooling Blocked

2026-11-17. Buffering Implement accepted the explicit B0 authorization and read
the stage/repository instructions. Invocation succeeded using its configured agent
without model override. No source, manifests, dependencies, tests, benchmark files
or checkpoints were edited by Implement. Rust 1.95 MSRV change is authorized but
not applied; manifest still declares 1.88.

Fresh read-only Git inspection: HEAD 6b2d3aba001c78504f4029a85c199d6e3d920161.
Only tracked pre-existing change: .github/agents/buffering-docs.agent.md adds
rust/cargo-info and rust/cargo-search selectors (one insertion/one deletion).
No untracked Rust/test/benchmark source. Existing design/checkpoint documents
under this directory are untracked and preserved. No source diff for Review.

Implement reported exact executed command (read-only):

```text
git status --short --untracked-files=all && git rev-parse HEAD && git diff --stat && git diff && git ls-files --others --exclude-standard
```

Configured CI compiler 1.97.0 and formatter nightly-2026-07-25; installed compiler
availability unverified. Implement could not load deferred Rust MCP tools because
its invocation lacked tool_search. Lead attempted its own tool_search for Rust MCP
cargo-check/test/add/build; result: "Found 0 tools." Required pre-edit cargo-check
and all subsequent Cargo gates are BLOCKED, not run or passed. No shell Cargo
fallback, toolchain installation, prototype or measurement was attempted.

No independent Verify capture or stage Review is started: there is no patch or
data to validate. Next prerequisite is restored Rust MCP discovery/execution (or
explicitly authorized alternative tooling), not renewed B0 scope approval. Resume
with fresh status, installed-toolchain inspection and MCP cargo-check
all_targets=true, all_features=true, toolchain=1.97.0, locking_mode=locked; then
the focused unchanged-wire/counter slice. All baseline metrics remain unmeasured.