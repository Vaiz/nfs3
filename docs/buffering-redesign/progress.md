# Buffering Status

- Scope: B0 baseline-only instrumentation with rallocator/Rust 1.95 and B1-B4
  document corrections authorized. No buffering behavior/prototypes or numeric policy.
- Prerequisite: [PR #171](https://github.com/Vaiz/nfs3/pull/171), `rust-msrv-1.95`
  -> `main`, is merged at `3ac587712affd8d7170d481f590dad496aca7934`.
  All 11 PR-head CI checks passed, including MSRV 1.95 and all four target
  debug/release lanes. Scope is only MSRV and three equivalent duration spellings.
  Head commit `6737e624ce89a16272fe04f91346b4d837380e0e`;
  worktree `/home/vaiz/repos/my/nfs3-rust-msrv-1.95` is clean.
  User merged; the assistant did not merge or enable auto-merge.
  Primary B0.1 source is preserved, but dirty `bufs` does not contain the merge.
- Active B0.1 source: detached `/home/vaiz/repos/my/nfs3-b0-baseline` at merged main,
  six tracked modifications and three untracked approved files. Host gates pass
  except deny; independent focused capture passed. Primary docs are canonical.
- Active prerequisite: user approved a separate cargo-deny policy/advisory fix PR
  with the [exact allowlist](baseline.md#outstanding-gates), no advisory ignores.
  `fix-cargo-deny` in `/home/vaiz/repos/my/nfs3-cargo-deny` is implemented with
  passing host gates, including deny; [PR #173](https://github.com/Vaiz/nfs3/pull/173)
  is open at `f7ee3746d7988481ce6051ace37b4789933866c7`.
  Final five-file scope and clean worktree verified: default Jiff timestamp
  display, no custom formatter/tests or example test target.
  Current-head CI snapshot: nine passed including MSRV, two macOS checks pending,
  no failures. No merge or auto-merge.
- Design: [D3.3](design.md), condensed; not independently accepted.
- Research: [pinned evidence](research.md) available; no measured NFS results.
- Review: [B1-B4](review.md) remain proof/acceptance obligations after two rounds.
- Baseline: [B0.1 focused verification](baseline.md#b01-active-verification-evidence)
  passed independently; MSRV-induced Clippy causality and deny attribution are
  established. Coupled duration fixes are included in the separate prerequisite PR.
- Prerequisites: Docs supplied an [online-source API handoff](research.md#b01-recorder-api-handoff).
  Prerequisite strict Clippy and deny pass. B0 still needs the prerequisite merged
  and adopted to clear its [dependency gate](baseline.md#outstanding-gates). CI does not validate
  the additional B0 allocator dependencies or full baseline workloads.
- Next: user merges PR #173 after CI; then refresh refs and reconcile B0 onto it.
  No automatic stash/reset/rebase of primary or changes to B0 evidence inputs.
  Baseline-only authorization does not authorize buffering behavior changes.

Keep this as a current status summary, not a stage or session history.