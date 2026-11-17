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

## Current Blocker

No B0 source patch or measurement exists. Last source snapshot was
`6b2d3aba001c78504f4029a85c199d6e3d920161`; refresh worktree before editing and
preserve unrelated agent changes. Installed toolchains and Rust checks are unverified.

Implement received Rust schemas marked deferred but no required loader; no Rust
call/server failure was observed. Resolve inconsistent tool exposure, then probe
cargo-metadata with workspace manifest, `no_deps: true`, offline locking if supported,
and run mandatory all-target/all-feature pre-edit check. Lead's tool visibility does
not establish worker availability. Do not retry unchanged setup or keep failure logs.