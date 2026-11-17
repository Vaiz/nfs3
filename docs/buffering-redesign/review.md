# Open Review Findings

Last independent architecture verdict: REVISE after two rounds. No third unresolved
design loop. User approved correction directions below; D3.3 documents them but
does not constitute independent acceptance or proof. Behavior remains unauthorized.
Resolved findings and obsolete proposals are not retained.

| ID | Remaining proof/check |
| --- | --- |
| B1, high | Incremental receive can deadlock: two 8 MiB messages can each hold 5 MiB of a 10 MiB R share. Prove completion-safe admission before every grant, including read-ahead/future fragments. Test N records >R/N without eager allocation. Numbers are counterexamples, not defaults. |
| B2, high | Owned Payload + work token must move through blocking/owned VFS WRITE without forced copy; retain accounting until actual work ends. |
| B3, medium-high | Accounted writable READ buffer must allocate before backend work and expose only initialized prefix. Prove short-read clamping/EOF/error behavior; external Vec fallback cannot claim pre-allocation bounds. |
| B4, medium | IP:port/XID is not reconnect deduplication. Prove admitted mutations survive disconnect with ownership/token, and define conflicting XID, uncertain duplicate and outcome-slot exhaustion. Review digest collision/cost and cache/eviction policy. |

Clarifications retained in design: final-drop callbacks outside reentrant domain
locks and correct shutdown; pre-IO client guarantees only with finite reply bounds;
explicit Rust 1.95/rallocator placement in B0. These still need implementation proof.

Baseline-only work is independently authorized under [baseline](baseline.md):
hooks absent by default, measurement-only allocator, comparable compiler/allocator/
harness lanes, honest unsupported-case reporting. No architecture/code/performance
pass. User agreement remains required for numeric resource/performance policy,
pool retention and transaction outcome/cache choices.