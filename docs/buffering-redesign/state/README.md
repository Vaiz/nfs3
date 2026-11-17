# Agent Checkpoints

Current resume state only. The lead is the sole checkpoint writer.

| Agent | Checkpoint |
| --- | --- |
| Buffering Lead | `lead.md` |
| Buffering Docs | `docs.md` |
| Buffering Review | `review.md` |
| Buffering Implement | `implement.md` |
| Buffering Verify | `verify.md` |

## Save and Resume

- Before delegation, overwrite the worker checkpoint with task, scope, input
  revisions, and `in_progress`; pass its path and only necessary artifacts.
- Workers return concise `Resume State`; the lead saves it before dependent work.
- On resume, read `lead.md`, then only checkpoints needed for the active task.
  Old `in_progress` means interrupted/unknown: inspect evidence before retrying.
- Changed inputs invalidate affected checks and approvals. Checkpoints never grant
  authorization. Preserve active user gates and unresolved findings.

## Retention Policy

Do not collect or retain historical data without a concrete current use. No session
diaries, transcripts, repeated tool failures, superseded designs, resolved review
rounds, or duplicate evidence. Replace state instead of appending. Keep only the
current contract, authorization, blockers, next action, and evidence needed to
reproduce an active baseline, validate a decision, or resume safely. Link evidence
once; prune it when its purpose ends. Never store credentials or private reasoning.

Decisions belong in `../design.md`; stage status belongs in `../progress.md`.

## Checkpoint Format

```markdown
# <Agent> State

- Task: <unique task/stage ID and scope>
- Status: pending | in_progress | blocked | completed
- Inputs: <design revision, code revision/worktree changes, relevant artifact paths>
- Result: <current outcome and evidence links, if any>
- Blockers: <unresolved findings or missing prerequisites>
- Next: <one concrete action>
```