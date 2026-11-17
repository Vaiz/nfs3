# Buffering Redesign Brief

- Redesign NFS3 buffering and deserialization to reduce payload copies and
  peak/live/retained memory without huge eager allocations from advertised lengths.
- Public API compatibility is not a goal. Valid RPC/XDR/NFS3 wire interoperability is.
- Shared buffers (`bytesbuf`, `bytes`), borrowing, and segmented storage are
  candidates, not mandated solutions. Account for retained backing allocations;
  a selective copy can be preferable to nominal zero-copy behavior.
- Draw on established crate implementations, not just novel abstractions.
- Distinguish source evidence, measurements, and hypotheses. Missing evidence is
  an explicit gap, not a passing result.

## Resume State

Read only the checkpoint supplied with your task and the relevant linked artifacts.
Check its scope and input revisions before reusing results; stale state is not
authorization. Return a concise `Resume State` section with completed work, evidence,
open issues, and the next action. The lead persists checkpoints; specialists do not
edit them or load other agents' state unless the task specifically requires it.