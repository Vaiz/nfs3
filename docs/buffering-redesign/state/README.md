# Agent Checkpoints

Persistent working state for the buffering redesign. Checkpoints are created on
first use; no research, design, or implementation has started merely because this
directory exists. The lead is the sole checkpoint writer, including for read-only
specialists.

| Agent | Checkpoint |
| --- | --- |
| Buffering Lead | `lead.md` |
| Buffering Docs | `docs.md` |
| Buffering Review | `review.md` |
| Buffering Implement | `implement.md` |
| Buffering Verify | `verify.md` |

## Save and Resume

1. Before delegation, save the task ID, scope, input revisions, and status
   `in_progress` in that agent's checkpoint. Pass its path and the relevant input
   artifacts to the agent, not the entire state directory.
2. The agent reads its checkpoint and checks that it matches the current task and
   inputs. It returns a `Resume State` section with completed work, evidence,
   unresolved issues, and the next concrete action. It does not write checkpoints.
3. After the agent returns, the lead persists that summary before starting the next
   dependent task. Use `completed` only for the assigned task, not the whole project;
   otherwise use `blocked` or `pending`. Preserve unresolved findings and durable
   evidence links when replacing an older task's checkpoint.
4. On restart, the lead reads `lead.md` first and only the other checkpoints needed
   for the active stage. An old `in_progress` status means interrupted or unknown,
   not proof that an agent is still running. Inspect the worktree and evidence
   before resuming or repeating actions.
5. Revalidate results against their recorded design/code revisions. Changed inputs
   invalidate affected approvals and checks. State records do not grant permission
   to implement, install tools, or expand scope; preserve the existing user gates.

Keep checkpoints concise. Link durable artifacts rather than embedding transcripts,
large diffs, or logs. Never store credentials or private reasoning. Design decisions
belong in `../design.md`; project-wide stage status belongs in `../progress.md`.
Those documents are created by the workflow when needed.

## Checkpoint Format

```markdown
# <Agent> State

- Updated: <ISO 8601 timestamp>
- Task: <unique task/stage ID and scope>
- Status: pending | in_progress | blocked | completed
- Inputs: <design revision, code revision/worktree changes, relevant artifact paths>

## Completed
<Verified outcomes, decisions relevant to this role, and changed paths>

## Evidence
<Source links or exact checks/results and durable artifact paths>

## Open Issues
<Unresolved findings, missing evidence, and required user decisions>

## Next Action
<One concrete step and the prerequisites needed to take it>
```