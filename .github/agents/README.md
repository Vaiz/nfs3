# Buffering Team

Select **Buffering Lead**. Default is design-only; explicitly authorize each code
stage. Current authorization/status: [progress](../../docs/buffering-redesign/progress.md).

| Agent | Configured model | Role |
| --- | --- | --- |
| [Lead](buffering-lead.agent.md) | GPT-6 Astra | Design, delegation, checkpoints |
| [Docs](buffering-docs.agent.md) | GPT-6 Luna | Read-only primary-source research |
| [Review](buffering-review.agent.md) | Claude Opus 5.5 | Independent design/code review |
| [Implement](buffering-implement.agent.md) | GPT-6.1 Sol | Sole Rust/test/benchmark writer |
| [Verify](buffering-verify.agent.md) | GPT-6.1 Terra | Independent baseline/checks |

Frontmatter owns models/tools; availability must be checked, never silently
substituted. Implement is also user-invocable; specialists cannot delegate.
Tool restrictions are capability boundaries, not a security sandbox.

Workflow: research -> design -> measurement assessment -> review -> user approval
-> baseline -> bounded implementation -> independent verification -> review.
Missing instrumentation gets a baseline-only stage; architecture changes return
to review. Never run multiple writers or benchmarks concurrently.

All agents read the [brief](buffering/brief.md); Implement/Verify also read
[Rust rules](buffering/rust-validation.md). The lead owns
[checkpoints](../../docs/buffering-redesign/state/README.md). Keep current state
and necessary evidence only, not histories or full research dumps in handoffs.