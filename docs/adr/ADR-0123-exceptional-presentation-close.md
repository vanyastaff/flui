# ADR-0123: Exceptional presentation close withdraws authority before retaining ownership

- **Status:** Accepted
- **Date:** 2026-10-05
- **Supersedes:** [ADR-0048](ADR-0048-frame-transaction-boundary.md), in part:
  its remaining-element disposal policy, during a terminal presentation close
  that holds a failure or runs inside an unwind. Per-element removal and frame
  recovery outside that close are unchanged.
- **Related:** [ADR-0127](ADR-0127-exceptional-path-retention.md) (which values
  are retained and which are released on an exceptional path)

## Context

Closing one presentation must revoke its capabilities even when a lifecycle,
platform or retirement callback panics. Catching that panic ends the ambient
unwind, so a subsequent opaque destructor can compete with the saved failure.
Retaining a tree without withdrawing its keys and executable handles would
leave a closed presentation reachable through a sibling's shared realm.

## Decision

The host carries the first failure explicitly through terminal close. An active
outer unwind also selects preserving mode. Authority withdrawal commits before
optional user notifications: presentation input targets and cached routes,
liveness, graph creation and writes, external build inbox and rebuild handles,
realm and local GlobalKeys, agent ports, focus, text input, gestures and mouse
tracking become unavailable for the closed owner. Shared sibling capabilities
remain usable. Closed graph operations report `SignalError::OwnerClosed`.

Required platform cleanup, including IME disable and accessibility withdrawal,
is attempted after failure. Key owners are moved out of their registries while
guarded and dropped only after every guard is released.

Once a failure is held, or the close runs inside an unwind, the close is in
preserving mode: optional widget disposal is skipped, and the closing
presentation's remaining user-owned values (its element, render and layer
trees, and the callbacks and captures they hold) are retained rather than
dropped, per ADR-0127. Retention follows authority withdrawal, so a retained
value can no longer be reached through the realm. The platform window and the
accessibility bridge are framework-owned and are released in preserving mode
as in a healthy close; only an unwind already in progress when they are
released retains them. Later failures and panic payloads cannot replace the
first failure. Healthy close drops its owners normally. Healthy closed-owner
rejection remains ordinary; an unrelated rejection unwind does not permanently
change that owner's terminal policy.

An ordinary `LifecycleSource::drain` still attempts all eligible callbacks,
including reentrant terminal callbacks, before resuming its first local panic.
The host suppresses subsequent optional terminal rounds only after that failure
returns to it, or when it already holds a failure. Required lifecycle commits
and the closing ladder still finish.

## Verification and limits

`presentation_close_retirement_failures_preserve_focus_ime_and_siblings`, a row
of `realm_and_presentation_isolation_matrix`, runs each case in a child
process: competing failures, a close during an unwind, release of the window
and bridge, callback reentry, saved routes, key reuse and a sibling's next
frame.

A single user value whose own destructor panics twice before reaching a catch
boundary still aborts; no container can catch that. Values outside the closing
presentation (the pipeline, post-frame callbacks, writers and other realm
fields) follow their own ownership rules. The render-demand retry policy is
unchanged.
