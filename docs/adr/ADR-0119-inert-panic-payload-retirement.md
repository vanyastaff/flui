# ADR-0119: Release known inert panic payloads while retaining opaque failures

- **Status:** Accepted
- **Date:** 2026-10-04
- **Supersedes:** [ADR-0104](ADR-0104-borrowed-notification-and-opaque-panic-retention.md)
  decision 4's unconditional payload retention, and the corresponding payload
  policy retained by [ADR-0109](ADR-0109-notification-channel-surface.md)
  decisions 3–4. Callback snapshot retention and first-failure priority remain binding.

## Context

The rendering layout boundary retained every caught panic payload before
reporting. This contains hostile payload destructors, but also leaked the boxed
static string from an ordinary layout panic. Miri's main-branch run completed
the assertions and then rejected that allocation leak. A formatted panic can
similarly leak its owned string and buffer.

Dropping arbitrary payloads inside another catch is not a repair: an aggregate
can panic from two field destructors and abort before the catch regains control.
Known standard text payloads have no such user-controlled destruction.

## Decision

Foundation's existing `panic::retain_opaque_payload` operation releases a
discarded payload only when its exact dynamic type is `&'static str` or
`String`. Those types' destruction cannot invoke user code. Every other
payload remains deliberately leaked without invoking its destructor. No
heuristic about a payload's size, fields, text or `needs_drop` relaxes this rule.
A propagated original failure still uses `resume_unwind`.

The leaf, recursive Box and recursive Sliver layout boundaries keep the caught
source payload behind `ManuallyDrop` while borrowing its diagnostic text and
invoking the separately contained tracing subscriber. After that borrow ends,
they transfer it to the shared operation. Secondary reporting payloads use the
same operation and do not replace the original `RenderError::Poisoned`.

This changes payload retirement only. Callback captures, queued obligations,
render-object destruction and arbitrary aggregate failures retain their existing
ownership policies. Containment still cannot recover an abort during user code
or destruction before control returns to the boundary.

## Verification

The existing Miri-running `frame_panic_containment_matrix` executes ordinary
static-string and owned-string layout panics through the real leaf boundary.
Miri leak checking must remain enabled. Restoring unconditional retention must
reproduce a leak after the otherwise successful matrix.

The existing subprocess family `layout_reporting_retains_opaque_payloads`
continues to exercise hostile aggregate payloads, competing diagnostic failures
and a healthy next layout across leaf, Box and Sliver paths. Retaining opaque
values must not be replaced by attempted arbitrary destruction. Execution
results belong to the task's validation report, rather than being assumed here.
