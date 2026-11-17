# B0 Baseline-Only Contract

User authorized B1-B4 document corrections and baseline-only instrumentation with
rallocator/Rust 1.95 on 2026-11-17. This remains valid. No buffering behavior,
prototype, resource policy or performance threshold is authorized. Architecture
review blockers do not prevent this separate unchanged-behavior stage.

## Allowed Changes

- Implement is sole Rust/test/benchmark writer; Verify independently captures and
  Review inspects actual patch/evidence. No concurrent writers or benchmarks.
- Raise workspace MSRV to 1.95 and directly affected docs/checks. Existing CI 1.97.0
  and nightly formatter need not change. No unapproved toolchain installation.
- Pin rallocator 0.4.0 and compatible recording dependencies at workspace level,
  used only by an opt-in nfs3_tests measurement executable/bench. Verify APIs/licenses.
  No product allocator replacement, bytesbuf prototype or unrelated dependency updates.
- Feature-gated hooks at existing receive/growth/codec/send/backend/test-copy sites,
  absent by default. No scheduling/allocation/ownership/VFS/IO/retry behavior changes.
- Reuse existing perf/tests; add a dedicated executable only for allocator isolation
  or structured output. Test tracker consistency and hook-on/off golden wire bytes.

## Measurement

Follow [validation-plan](validation-plan.md). Report process allocation events and
explicit protocol-origin/copy/zero-fill hooks with honest attribution. Address is
not lifetime identity; capacity change alone does not prove growth-copy bytes.
Unknown movement stays bounded/unknown, not zero. Retained output lasts to actual
drop. Baseline has no new pool/read-into features: label them absent/unsupported.

Use identical compiler/allocator/harness per comparison lane; keep diagnostic
rallocator and normal-allocator timing separate. Record reproducibility-critical
inputs, raw samples, certainty and overhead. Harness workload/time limits are not
product policy. No deliberately OOM-inducing advertised-length probes or hangs.

## First Slice and Acceptance

Start with deterministic opaque encode/decode or existing RPC: known payload,
copy/initialization counters and golden bytes must agree with unchanged code;
feature-disabled behavior must match. Run immediately after first substantive edit.
Then cover metadata RPCs, 4/64/256/1024 KiB READ/WRITE, retained outputs/handles,
directories and safely bounded fragmentation/short-read/slow-peer/concurrency cases.
Unsupported old-client/cancellation workloads are correctness results, not speedups.

Acceptance: unchanged default behavior/wire bytes, opt-in instrumentation/allocator,
validated counters, independent reproducible capture, relevant Review acceptance
and repository gates. Missing required metrics/gates block completion. No speedup
is expected or claimed. If exact instrumentation requires redesign, stop at the gap.

Use [Rust rules](../../.github/agents/buffering/rust-validation.md); shell Cargo,
tool installation and commits/branches are not granted. Running an MCP-built
measurement executable for authorized capture is allowed; record command/settings.

## B0.1 Active Verification Evidence

Verify independently passed the focused slice; full B0 and repository gates remain
blocked; [current review obligations](review.md#b01-patch-review) remain.
Current capture worktree: `/home/vaiz/repos/my/nfs3-b0-baseline`, detached at
`3ac587712affd8d7170d481f590dad496aca7934`, nothing staged.
Stage modifies Cargo.toml/Cargo.lock, crates/nfs3_tests/Cargo.toml,
crates/nfs3_types/Cargo.toml and crates/nfs3_types/src/xdr_codec/{mod,opaque}.rs.
The prerequisite MSRV/duration changes are already in this base, not in the stage diff.
Untracked stage source: crates/nfs3_tests/src/bin/opaque_allocator_baseline.rs,
crates/nfs3_types/src/xdr_codec/opaque_diagnostics.rs and
crates/nfs3_types/tests/opaque_baseline.rs. All nine source blobs match the preserved
primary worktree. Tracked diff is 80 additions/no deletions; SHA-256
`b5f21cc1201bbdce3ec89314084d6f36326c2c9c66dd46f52355a880be1a5792`.

### Reproduction

Independent Rust MCP calls used manifest `/home/vaiz/repos/my/nfs3-b0-baseline/Cargo.toml`,
toolchain `1.97.0`, and `locking_mode: "locked"` (metadata used `"frozen"`):

| Tool | Additional arguments | Result |
| --- | --- | --- |
| cargo-metadata | `no_deps: true` | Pass |
| cargo-test | `package: ["nfs3_types"], test: "opaque_baseline", test_args: ["--nocapture"]` | 3 passed |
| cargo-test | Same, `features: ["opaque-diagnostics"]` | 5 passed |
| cargo-test | `package: ["nfs3_types"], lib: true, features: ["opaque-diagnostics"], testname: "opaque_diagnostics", test_args: ["--nocapture"]` | 2 passed |
| cargo-test | `package: ["nfs3_tests"], bin: "opaque_allocator_baseline", features: ["allocator-diagnostics"], test_args: ["--nocapture"]` | 1 passed |
| cargo-build | Same package/bin/features, debug | Pass |
| cargo-clippy | `workspace: true, all_targets: true, all_features: true, warnings_as_errors: true` | Pass |
| cargo-fmt | `toolchain: "nightly-2026-07-25", all: true, check: true` | Pass |

`git diff --check` passed. Implement's pre-edit check, broader workspace all-feature
tests, all-target/all-feature build and machete passed but were not independently rerun.

Capture command: `/home/vaiz/repos/my/nfs3-b0-baseline/target/debug/opaque_allocator_baseline`,
once, no arguments,
inherited environment. Linux x86_64, kernel `7.3.4-1-cachyos`, Intel i7-13700K,
24 logical CPUs, page size 4096. Rust 1.97.0 `2d8144b78`; formatter nightly
`da86f4d07`. Debug/single-thread, rallocator 0.4.0/seismograph 0.5.0; no seed/timing.

```text
B0.1 diagnostic; capacity_per_thread=64; stacks=false; stopped=true
events=6 retained=6 lost=0 threads=1
thread=1 sequence=1 kind=Allocation address=140212201717760 correlation=140212201717760 size=37 alignment=8
thread=1 sequence=2 kind=Deallocation address=140212201717760 correlation=140212201717760 size=37 alignment=8
thread=1 sequence=3 kind=Allocation address=140212201734144 correlation=140212201734144 size=73 alignment=8
thread=1 sequence=4 kind=Deallocation address=140212201734144 correlation=140212201734144 size=73 alignment=8
thread=1 sequence=5 kind=Allocation address=140212201799696 correlation=140212201799696 size=1 alignment=1
thread=1 sequence=6 kind=Deallocation address=140212201799696 correlation=140212201799696 size=1 alignment=1
opaque=Ledger { encode_payload: Count { attempts: 1, attempted_bytes: 1, completions: 1, completed_bytes: 1 }, encode_padding: Count { attempts: 1, attempted_bytes: 3, completions: 1, completed_bytes: 3 }, decode_payload: Count { attempts: 1, attempted_bytes: 1, completions: 1, completed_bytes: 1 }, decode_padding: Count { attempts: 1, attempted_bytes: 3, completions: 1, completed_bytes: 3 }, payload_zero_fill: Count { attempts: 1, attempted_bytes: 1, completions: 1, completed_bytes: 1 }, padding_zero_fill: Count { attempts: 1, attempted_bytes: 4, completions: 1, completed_bytes: 4 }, lost_updates: 0 }
fixture=passed; initial_live_set=absent; lifetime_ids=absent; cross_thread_order=unvalidated
process_peak=unknown; retention=unknown; realloc_overlap=unknown; timing=not_measured
```

The executable checks the first four explicit fixture events; the last one-byte
pair is observed in this capture, not independently asserted by the fixture.
Counters describe logical IO/explicit initialization, not physical payload copies.
No exact global lifetime/peak/retention/realloc-overlap or instrumentation-overhead
measurement is established.

### Outstanding Gates

MSRV-induced `clippy::duration_suboptimal_units` failures are repaired in the
[separate prerequisite](progress.md), merged with passing CI and independent strict
Clippy. Revalidate the combined B0 worktree after safe reconciliation; prerequisite code
acceptance is not full B0 acceptance.

Deny exit 5: advisories/licenses failed; bans/sources passed. Implement recovered
the diagnostic output and confirmed all advisory versions in the base lockfile:

| Existing dependency | Advisory |
| --- | --- |
| anyhow 1.0.101 | RUSTSEC-2026-0190 |
| chrono 0.4.43 | RUSTSEC-2026-0543 |
| crossbeam-epoch 0.9.18 | RUSTSEC-2026-0204 |
| intaglio 1.12.0 | RUSTSEC-2026-0078 |
| tokio 1.49.0 | RUSTSEC-2026-0386 |

No advisory named the allocator/recorder additions. Of 120 license-rejection
diagnostics, new rallocator 0.4.0, seismograph 0.5.0 and seismograph_rallocator
0.5.0 are MIT rejected as not explicitly allowed. Existing windows-sys 0.61.2
is MIT OR Apache-2.0 and similarly rejected; only a new dependency edge was added.
There is no repository deny policy; no policy has been invented or failure waived.
Reconciled diagnostics report no unlicensed, unknown-license, clarification or
gathering errors. Remaining expressions can use MIT/Apache-2.0 alternatives;
BSD-3-Clause, Unicode-3.0 and Unlicense also appear. LGPL/Zlib/LLVM-exception
occur only in alternatives and need not be approved to satisfy those expressions.

**Owner-approved policy/scope (implemented in [PR #173](https://github.com/Vaiz/nfs3/pull/173),
not yet adopted by this B0 capture):** allow exactly `MIT`,
`Apache-2.0`, `BSD-3-Clause`, `Unicode-3.0`, and `Unlicense`, and prepare a separate
prerequisite PR remediating the five reported existing dependency advisories.
No advisory ignores, blanket exceptions or policy weakening are authorized.
Verify actual advisory sources/patched versions before selecting dependency changes.
For the example-only chrono replacement, the user accepts Jiff's default timestamp
display. No custom compatibility formatter or formatting tests are wanted.
See [Docs semantics](research.md#dependency-gate-policy).

MSRV 1.95 and Windows x86_64/Linux aarch64/macOS aarch64 targets
are unavailable. CI debug/release, test_mirror_fs and independent peer
interoperability were not run in this slice. Full B0 workloads, memory metrics,
overhead, Review and numeric criteria remain outstanding.

Refresh actual source/status before reusing this evidence; changed inputs
invalidate affected checks. Current next action is in [lead state](state/lead.md).