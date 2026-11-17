# Independent Measurement Assessment

## D3-V4: PASS for Second Review Submission

Verify accepted D3.1 measurement-plan feasibility for submission to independent
Review, not implementation, numeric limits or performance claims. Clarifications
carried into D3.2: detached/blocking work keeps suppression until actual termination;
outcome entries retain request identity; SYSTEM_ERR does not communicate outcome
uncertainty; exact allocator reconstruction needs validated cross-thread ordering/
address reuse as well as loss-free events; explicit incremental huge-length, tiny
fragment, overflow and concurrent slow-peer/cancellation tests.

No builds, measurements, edits, installs or new Git inspection. Prior D3-V3 snapshot
remains historical. Current policy/defaults/gates require user agreement. Baseline
instrumentation and later prototype remain separate authorizations. D3.2 applies
the requested clarifications and goes to Review, not an assumed code-validation pass.

## D3-V3: REVISE, 2026-11-17

Verify reviewed design-v3.md, D3 research and R1-R11 without builds, benchmarks,
installs or edits. Plausible design, but exact accounting/admission and retry
lifetime needed corrections: live/free disjoint, retained a subset of live, atomic
transitions/trim; no hard bound established by post-allocation check; explicit
progress and disabled-deadline behavior; suppression retained while backend runs;
duplicate identity/replay classification/write-not-delivery; compile/proof stages;
validated loss/origin/realloc/copy/overhead instrumentation.

D2 numerical gates are WITHDRAWN, including those preserved in historical reports.
Baseline-only instrumentation requires authorization; provider/transport prototype
is a separate later authorization. User policy/overload priorities and measured
acceptance criteria remain undecided. No feasibility or implementation pass.

Fresh read-only Git calls: git rev-parse HEAD; git status --short --branch
--untracked-files=all; git diff --stat HEAD; git diff --name-only HEAD -- crates
Cargo.toml Cargo.lock. HEAD 6b2d3aba001c78504f4029a85c199d6e3d920161. No tracked
source diff or untracked source; unrelated agent-file modification and untracked
design/checkpoint documents reported, not altered. Specialist did not return the
agent filename; no exact path inferred. This is a snapshot, not continuous status.

## Current User Directions C2

As of 2026-11-17, performance gates are not set. The user withdrew the lead's
numerical restrictions and requires asking before imposing new ones. The D1/D2
assessments below are historical evidence, not an active acceptance contract.
Investigate optimization opportunities rather than stopping at modest targets.
Pools are allowed; distinguish live payload, retained views, reusable pool storage
and allocator overhead instead of requiring zero pool retention. Unsafe
initialization is allowed with soundness evidence and measured benefit against a
safe alternative. MSRV increases and VFS API changes are allowed; bytesbuf is the
recommended candidate. Reassess instrumentation and comparisons for the revised
design, then propose criteria for user agreement before behavioral implementation.

## D1-V1: REVISE

Buffering Verify assessed design D1 on 2026-11-17. No benchmarks, builds, or edits.

Required amendments:

- The listed budget is 7.625 MiB, leaving 384 KiB within 8 MiB. Prove simultaneous
  lifetimes, task/channel/descriptors/control overhead and client returned-owner
  credit lifetime. Identify process overhead outside record/connection reservations.
- For B>0 body bytes, origins N = 1 + ceil(max(0, B-4096)/65536); N=0 for B=0.
  At 2 MiB, N=33 and nominal data capacity is 2 MiB + 4 KiB. Actual allocation
  capacity and allocator overhead must not be assumed equal to nominal requests.
- Audit explicitly fallible allocation sites. Bytes control blocks and runtime
  tasks/channels can allocate infallibly; specify failure behavior and measurement.
- Provisional allocation gates: large receive <=3N+8 backing/control allocations
  plus separately measured decoded metadata; metadata <=baseline+8 per operation.
  Realloc peaks count both old and new storage. Calibrate before behavior changes.
- Rejected oversized first marker: zero body allocation; later rejected marker:
  no additional allocation. Fragment count cap does not permit 2 MiB of one-byte
  sender fragments. Network delivery sizes are a separate workload dimension.
- Tiny retention assertions must match detachment policy, test exactly 4x and
  cross-chunk boundaries, and separate data capacity from control allocations.
- Peak <=75% and perf ratios remain provisional until comparable baseline exists.
  Cooperative cancellation cannot guarantee termination of blocking/non-yielding
  backend futures; state the boundary and test yielding cleanup.

Baseline-only instrumentation requires explicit later authorization: test-scoped
allocation identity/deallocation/reallocation tracking; capacity/control categories;
origin lifetime and unique live/retained capacity; explicit copy/zero-fill hooks
possibly feature-gated in production paths; deterministic workload harness in
existing test/perf files. No protocol behavior changes during instrumentation.

Pairing: 20 serial paired release runs, alternate baseline/candidate order, warmup,
>=10,000 completed ops per latency sample. Bootstrap paired run-level ratios with
10,000 resamples. Require 95% CI lower throughput >=0.95 and upper p99 <=1.10;
interval width <=5 percentage points, otherwise inconclusive. Keep raw samples
and timing boundaries. Instrumented/uninstrumented paired overhead >3% throughput
or >5% p99 requires uninstrumented timing with separately reported counters, never
mathematical correction. Equivalent completed workloads only for performance ratios.

Existing perf bench has one Tokio duplex client; ReadFs copies backend data. Tests
have fixtures/basic truncation but lack adversarial segmentation/lifetime coverage.
RPC helper only handles single fragments; old client rejects fragmented replies.

Required future gates: locked CI debug/release Linux x86_64/aarch64, Windows
x86_64, macOS aarch64; MSRV 1.88; Tokio/smol; nightly fmt, Clippy, tests, build,
machete and deny. WASM unestablished. Independent RFC fixtures/parser/captures and
an independent NFS peer remain required. CI runs test_mirror_fs debug even in its
release job; build through MCP first. Peer and execution-tool availability unknown.

### Workspace Evidence

Verify's terminal inspection succeeded, unlike lead tool discovery:

- HEAD: `6b2d3aba001c78504f4029a85c199d6e3d920161`, branch `bufs`.
- Calls: `git rev-parse HEAD`, `git status --porcelain=v1 --untracked-files=all`,
  `git status --short --branch --untracked-files=all`, `git diff --stat HEAD`,
  `git diff HEAD --`.
- Tracked diff empty; only untracked design.md, progress.md, research.md and
  state/docs.md, state/lead.md, state/verify.md under this document directory.
  No untracked source paths at assessment time. Later document additions are not
  covered by this snapshot; do not treat it as a new status inspection.
- Rust MCP tools advertised to Verify, but required discovery tool absent in its
  invocation. MCP execution/toolchain availability unverified, not successful.

## User Measurement Recommendation

User recommended `rallocator` on 2026-11-17. Research D1-R2 will establish identity,
version, license/MSRV/target fit, measurement semantics, overhead and limitations
before choosing integration. No package installation or source edits authorized.

## D2-V2: ACCEPT for Measurement-Plan Scope

Buffering Verify assessed design D2 against D1-R2 research and D1-V1 findings on
2026-11-17. Numerical thresholds accepted as provisional, not measured or locked.
The isolated rallocator diagnostic lane and separate normal-allocator performance
lane are feasible proposals, not validated integration. No required design
corrections remain within this assessment's scope; proceed to design review.

Before behavior changes: authorize baseline stage, validate loss-free allocation,
lifetime, copy and realloc accounting, then lock thresholds. Harness compatibility,
allocator overhead, independent-peer coverage and target-specific gates remain
unverified. Inability to measure a required metric blocks its gate.

No new Git inspection, builds, benchmarks, edits or installs. The D1-V1 Git snapshot
above remains the evidence, not a fresh inspection. Next: independent Review.