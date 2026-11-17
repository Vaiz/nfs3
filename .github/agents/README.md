# Buffering Redesign Team

This is a workspace-local VS Code Copilot setup for designing and implementing the
NFS3 buffering and deserialization replacement.

## Start

Select **Buffering Lead** in the Chat agent picker and send:

> Start the buffering redesign in design-only mode. Gather Luna's documentation,
> design the ownership and buffering architecture, have Terra assess the validation
> plan, and obtain Opus's review. Save the design and progress artifacts, then stop
> before Rust implementation.

After reviewing the resulting design, explicitly authorize implementation of that
revision or a specific stage. The lead then coordinates Sol, Terra, and Opus.

| Agent | Fixed model | Responsibility |
| --- | --- | --- |
| Buffering Lead | GPT-6 Astra | Architecture, decisions, delegation, progress |
| Buffering Docs | GPT-6 Luna | Primary-source dependency and protocol research |
| Buffering Review | Claude Opus 5.5 | Independent design and implementation review |
| Buffering Implement | GPT-6.1 Sol | Sole Rust, test, and benchmark writer |
| Buffering Verify | GPT-6.1 Terra | Independent baselines, tests, resource measurements |

Sonnet and Haiku are deliberately unassigned: this workflow does not need extra
roles or cheaper fallbacks for critical design and review work.

Model strings use VS Code's provider-qualified display-name format. They are
configuration, not proof of account availability. Before the first real run,
confirm each configured model resolves in your installation; if the picker uses a
different identifier, update the corresponding frontmatter to that exact identifier.
Do not silently substitute models. Specialists are subagent-only and cannot spawn
other agents. The lead's allowlist contains exactly these four specialists.

The MCP selectors use the `Rust` server configured in `.vscode/mcp.json`. Confirm
that it is running and that its selected tools resolve in the agent tool picker.
Missing models or tools must be reported, not treated as successful validation.
Tool restrictions are capability boundaries; behavioral rules such as "check-only"
and the single-writer rule are instructions, not a security sandbox.

## Agent Instructions

Each agent contains its own role-specific objectives, constraints, and output
requirements. All five load the short [shared brief](buffering/brief.md); only
implementation and verification load [Rust execution rules](buffering/rust-validation.md).
Research, design, review, and measurement procedures stay in their owning agents.
Handoffs provide relevant stage context, not every agent's instructions or the full
research archive. This README is a user guide, not an instruction dependency.

- [buffering-lead.agent.md](buffering-lead.agent.md): architectural goals, design
  decisions, delegation, authorization gates, and progress tracking.
- [buffering-docs.agent.md](buffering-docs.agent.md): source-backed research into
  `http_headers` and independently discovered crates, with applicability analysis.
- [buffering-review.agent.md](buffering-review.agent.md): independent criteria for
  accepting or rejecting designs and implementations.
- [buffering-implement.agent.md](buffering-implement.agent.md): scoped code changes,
  API migration, regression tests, and instrumentation.
- [buffering-verify.agent.md](buffering-verify.agent.md): reproducible baselines,
  resource-bound checks, repository gates, and evidence for review.

## Workflow

Research -> design -> measurement-plan assessment -> design review -> user
authorization -> baseline -> implementation -> validation -> code review.
Implementation, validation, and review repeat per stage. Missing baseline
instrumentation is handled before behavioral changes; architecture changes return
to design review.

The lead creates `docs/buffering-redesign/design.md` and
`docs/buffering-redesign/progress.md` when the workflow runs. These capture the
design decisions, authorized scope, evidence, review outcomes, and remaining work.

## Persistent State

[docs/buffering-redesign/state/README.md](../../docs/buffering-redesign/state/README.md)
defines per-agent checkpoints, created on first use. The lead saves assignments
before delegation and returned resume summaries afterward. Each specialist receives
only its own checkpoint and relevant artifacts; read-only agents need no write tools.
On restart, the lead resumes from its checkpoint and revalidates interrupted work.