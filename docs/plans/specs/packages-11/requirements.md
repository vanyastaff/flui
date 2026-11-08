# Requirements: shared capability execution and conformance

- **Status:** Accepted
- **Date:** 2026-11-08
- **Finding:** `packages-11`

| ID | Requirement |
|---|---|
| R1 | Linux portal providers use the host's bounded IO executor through `ProviderContext`; they do not create a `zruntime` thread per UI runtime. No process-global executor or bus is introduced. |
| R2 | Portal providers registered in one capability registry share one lazily reconnecting session-bus connection and one checked token sequence. Request cancellation and task bookkeeping remain runtime-local, so retiring one runtime does not cancel another's work. |
| R3 | Every accepted request has a terminal outcome on success, refusal, backend failure, panic containment and owner retirement. No resolver, user callback or captured destructor runs while a task-book or bus lock is held. |
| R4 | Launcher and file-dialog behavior is described by one typed conformance scenario table run against scripted providers and the real Linux portal provider on a private bus. Protocol/security tests remain separate. A multi-runtime regression proves shared connection, distinct tokens and independent cancellation. |
| R5 | `flui-system/ARCHITECTURE.md` states the actual ownership, execution, cancellation, panic and testing contracts. Dependency direction, platform parity and repository gates remain intact. |
