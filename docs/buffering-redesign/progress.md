# Buffering Redesign Progress

- Authorized scope: user approved B1-B4 design corrections and B0 baseline-only
  instrumentation using rallocator/Rust 1.95. See baseline.md; no behavior/prototypes.
- Current proposal: [design-v3.md](design-v3.md) D3.3; design.md preserves D2/C2 and
  links to it. Second review REVISE: eight original findings resolved in design;
  B1-B4 technical blockers remain. User permits MSRV increases.
- C2: pools allowed; unsafe initialization allowed with soundness and benefit
  evidence; contiguous VFS compatibility not a goal; bytesbuf is the user's
  recommendation, bytes is not preferred. Resource defaults should favor performance
  and low friction, with conservative configuration available. Performance gates
  are unset; D2 numerical restrictions are withdrawn. Ask before imposing any
  new restriction. These directions supersede conflicting historical proposals.
- Research: D1-R1/R2 and D3-R4/R6 completed; D3-R5 incomplete result preserved.
  Pinned current bytesbuf/pool/custom-block/std evidence in research.md.
- Baseline: not captured; B0-I1 authorized but blocked by unavailable Rust MCP
  discovery/execution before source edits. Details in baseline.md.
- Code revision: Verify inspected 6b2d3aba001c78504f4029a85c199d6e3d920161 on bufs;
  no source diff/untracked source; unrelated agent modification preserved in latest
  snapshot. Exact historical snapshot scope in validation-plan.md.
- Agent availability: all four registered with expected configuration; Docs,
  Verify and Review invocations succeeded without model overrides;
  Implement invocation also succeeded without override; Rust tools unavailable.

## Stages

1. Local receive/decode/send/async ownership inspection: completed (source evidence,
   not measurements).
2. External source research: completed, no files changed by specialist.
3. Architecture D3: proposed current bytesbuf/accounted pooling, segmented VFS,
   origin charges and explicit migration/lifetime/retry contracts.
4. Verify assessment: D1-V1 REVISE and D2-V2 ACCEPT (measurement-plan feasibility)
  saved in validation-plan.md as historical assessments. C2 withdraws numerical
  targets; changed inputs require reassessment and user agreement.
5. D3-V3 REVISE corrected; D3-V4 PASS for review submission with D3.2 clarifications.
  D3-RV2 second review REVISE persisted in review.md. R2/R3/R11 partial;
  R1/R4/R5/R6/R7/R8/R9/R10 resolved in design, not validated code.
6. User approved corrections and baseline-only stage. D3.3 documents corrections;
  B0-I1 returned tooling BLOCKED, no edits. Independent capture/review pending;
  behavior not started. Lead discovery returned no Rust tools as well.

## Changed Paths

Only design/coordination documents under `docs/buffering-redesign/`: research.md,
design.md, design-v3.md, validation-plan.md, review.md, progress.md and agent
checkpoints. No Rust, manifests,
lockfiles, agent configurations, or unrelated worktree changes made by lead.

## Validation and Unresolved Issues

Document diagnostics reported no errors for initial state/research documents.
No Rust build/test/lint, performance, memory or independent wire check run. Required
future gates are in D3.2. No performance improvement or implementation completion
claimed. Verify assessed plan feasibility; second Review found B1-B4. Numeric
restrictions/gates remain unset. No architecture pass.

## Next Action

Restore Rust MCP discovery/execution or obtain explicit alternative-tooling
authorization, then resume B0-I1. No source patch to send to Verify/Review yet.
Preserve unrelated docs-agent selectors. No active delegation. B0 remains authorized;
behavior/prototypes and new numeric restrictions remain outside that scope.