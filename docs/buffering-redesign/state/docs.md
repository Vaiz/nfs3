# Buffering Docs State

- Task: B0.4 pinned recorder checkpoint semantics.
- Status: completed
- Inputs: [existing recorder evidence](../research.md#b01-recorder-api-handoff);
  Oxidizer `6021c37b6b8e5459d3e487d75d1fb2d5834fc02b`, rallocator 0.4.0,
  seismograph 0.5.0. No new crate selection or dependency change.
- Scope: read-only authoritative source for repeated Retain snapshots followed by
  Stop, synchronous suppression of snapshot/decode/observer allocations and frees,
  cumulative sequence/loss validation, initialization and event-visible size limits.
- Result: existing API evidence supports bounded recording/Stop, but repeated Retain
  lifecycle tests, nested suppression and exact slot/metadata bytes were not
  independently verified. These gaps are explicit in research.md.
- Blockers: none for the chosen B0.4 single-Stop allocation-marker design, which
  avoids those unverified APIs. Initial process heap and actual realloc count/
  overlap remain unobservable; no exact claims.
- Next: reuse established single-Stop path; no further external research needed
  unless implementation finds a new dependency on the unverified semantics.
