# ADR-0241: A driver trace is a versioned JSONL directory that embeds the input recordings

- **Status:** Proposed
- **Date:** 2026-10-17
- **Related:** [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the input
  recording it embeds; its privacy rule and completeness rule are reused),
  [ADR-0238](ADR-0238-flui-driver-protocol.md)
- **Spec:** [flui-driver](../plans/specs/flui-driver/design.md), "Trace"

## Context

A person replays a session, an agent's run must be reviewable, and a failed CI scenario must be
reproducible. ADR-0215 records the input stream an app applied, and the native recording codec
records what an OS delivered. Neither records the driver commands, what was expected, or the
evidence. Playwright's `trace.zip` shows the value of keeping locators, an action log and
before/after snapshots per action. The repository already standardized on JSON Lines with a
versioned header (ADR-0215 §3).

## Decision

1. A trace is a directory `<name>.flui-trace/` with:
   - `trace.jsonl`;
   - `input.jsonl`, an ADR-0215 recording verbatim, when recorded;
   - `native/<backend>.jsonl`, in the native recording codec, when captured;
   - `evidence/<sha256>.<ext>`, content-addressed captures and tree snapshots.

   `flui drive trace pack` zips it for transport; readers accept both.
2. `trace.jsonl` opens with a header:
   `{"flui_trace": 1, "protocol", "target": {kind, backend, os, app, profile}, "started_unix_ms", "privacy"}`.
   Rows are tagged objects:
   - `command{seq, t_ms, method, params}`, with params as the caller wrote them, locators included;
   - `result{seq, t_ms, ok | error, evidence[]}`;
   - `event{t_ms, name, params}`;
   - `gap{rows, reason}`;
   - `end{}`.

   Unknown fields are ignored and an unknown row tag is an error, as in ADR-0215.
3. A trace is complete only when it ends with `end`. A writer that fails mid-run leaves an
   incomplete trace that readers report as incomplete. A full disk fails the write and never
   rewrites earlier rows. Evidence over the trace's budget (default 256 MiB) becomes a `gap` row.
4. Privacy follows ADR-0215 §5: typed text and text-bearing tree fields are redacted unless the
   session sets `keep_text`, recorded in the header. Captures cover the target's windows only.
5. The format version is `flui_trace`. A change that is not additive bumps it and names its ADR. A
   reader refuses a newer major version with the version it found.
6. Replay re-issues commands and compares results modulo volatile fields (handles renumbered by
   first appearance, times, frame ids). Codegen emits a Rust test from commands and results.
   Neither reads raw coordinates when a locator is present.

## Consequences

- The native-recording goldens are produced by `record.start{native}` instead of
  `FLUI_NATIVE_RECORDING_BLESS`, and the hold tests read them unchanged.
- The inspector's trace player and `flui drive trace replay` share the reader.

## Alternatives considered

- **Playwright-style single zip.** Rejected as the working form: a directory diffs in review, and
  appends are crash-safe. The zip is kept for transport.
- **Binary (`rmp-serde`).** Rejected for the same reasons as ADR-0215 §3. Evidence blobs are
  already binary files.

## Verification

The new driver crate's `trace` tests:

- the header and row golden;
- an incomplete trace detected;
- the budget producing a `gap`;
- redaction by default;
- replay of a recorded headless session reporting no divergence, and a deliberately changed app
  diverging at the expected `seq`;
- codegen output compiling and passing.
