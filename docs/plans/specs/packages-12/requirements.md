# Requirements: producer-owned frame telemetry

- **Status:** Accepted
- **Date:** 2026-11-08
- **Finding:** `packages-12`

| ID | Requirement |
|---|---|
| R1 | Frame, presentation, pipeline and animation tracing identities have one typed producer-owned vocabulary shared by every emitter and devtools consumer. |
| R2 | Correlation and timing field spellings used on both sides come from the same diagnostics contract; application spans and non-span events remain excluded. |
| R3 | The SDK exposes no `FrameSnapshot` without a public producer, and devtools has no snapshot timeline bridge without a production caller. Internal scheduler telemetry and its real overlay/tracing consumers remain. |
| R4 | Devtools documentation and doctests use SDK package dependencies plus external tracing crates, not the internal `flui-log` composition root. |
| R5 | Public contract tests and real frame end-to-end tests fail when a producer identity, partition field or commit/submit/animation span disappears. |
