# ADR-0161: Immutable owner-local gesture recognizers

- **Status:** Accepted
- **Date:** 2026-10-07
- **Supersedes:** [ADR-0086](ADR-0086-signal-writes-through-event-context.md) §4,
  only its promise to preserve gesture extension signatures
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md)

## Context

The synchronous gesture graph belongs to its input owner. Shared mutable
callback configuration obscured ownership: a builder-shaped setter could
replace a callback after a recognizer was shared, while a strong arena member
reference kept the recognizer alive until explicit disposal. An allocation-bound
`add_pointer` receiver also prevented heterogeneous recognizer dispatch.
The widget catalog repeated admission and event forwarding separately in its
gesture detector, back gesture and draggable implementations.

We choose immutable construction and weak attachments rather than retaining a
mutable shared callback API or requiring consumers to break lifetime cycles
with disposal. The extension trait is open because external recognizers are a
supported consumer, and arbitration owns the invariants an implementation must
not change.

## Decision

Each built-in recognizer exposes a builder that configures callbacks and gesture
settings before `build()` returns `Rc<Self>`. Built callbacks cannot be replaced.
Recognizer ownership, builders and executable attachment sets remain
`!Send + !Sync`; their methods execute synchronously on the owner thread.
Callback configuration uses ordinary owned fields, while contact state uses
owner-local interior mutation.

`GestureRecognizer: GestureArenaMember` and `GestureArenaMember` are open,
dyn-compatible traits. A recognizer receives `PointerDispatch<'_>` through
`add_pointer(&self, ..)` and `handle_event(&self, ..)`. Arbitration uses
`accept_gesture` and `reject_gesture`; an optional `deadline()` describes the
armed deadline and the arena calls `poll_deadline(now)` only when due.
External implementations use these same contracts rather than a marker-trait
bridge with a reduced deadline interface.

`ArenaMembership` binds an arena to the exact weak allocation created with
`Rc::new_cyclic`. `PrimaryContact` composes that membership with one admitted
sequence. Admission refuses overlap, non-Down input, invalid coordinates and
exhausted identity allocation. A `ContactId` distinguishes a new sequence from
an older one reusing the same device pointer. Its snapshot includes device kind,
both coordinate spaces and settings frozen at admission. Multi-contact
recognizers compose membership independently for each pointer rather than
inheriting a single-contact base class.

The arena stores weak member identities, including eager winners and pending
verdict recipients. Each notification upgrades its recipient immediately before
invocation and holds that owner for the call. A recognizer whose last owner was
released by an earlier callback is skipped. Exact generational entry handles
remain the preferred way to resolve a contest; direct arena resolution borrows
the caller's owning `Rc` and does not transfer lifetime responsibility.

Explicit `cancel() -> CancelOutcome` withdraws active work, delivers its
cancellation at most once and leaves the recognizer reusable. `cancel_all`
attempts every supplied recognizer before resuming the first failure.
Last-owner Drop is silent: it withdraws exact membership and releases any held
arena debt through deferred resolution, without invoking peers or gesture
callbacks inline. A held sweep remains deliverable after its holding
recognizer disappears.

State transitions and outgoing ownership commit before user callbacks, clocks,
diagnostics or destructors run. No state borrow spans user code. A reentrant
operation may admit a replacement sequence, so later work checks its original
contact identity before changing state or delivering another notification.
Containment keeps the first failure authoritative and retires or retains opaque
ownership according to ADR-0127. An outer boundary cannot rescue a single user
aggregate whose own destructors double-panic before returning to it.

`RecognizerSet` stores ordered weak attachments. A Down predicate controls only
new admission; subsequent events reach every live attachment, so changing a
predicate cannot strand an admitted terminal tail. Each attachment is contained
independently and later attachments still run before the first failure resumes.
`Listener::recognizer` and `recognizer_when` use this set. Widget state keeps the
strong owners. `GestureDetector`, `BackGestureDetector` and `Draggable` use this
production path instead of separate forwarding groups. Removing an attachment
mid-contact requires its owner to cancel that contact explicitly.

## Consequences and verification

The legacy recognizer base, inheritance chain, marker bridge, mutable
`with_on_*` configuration and recognizer `dispose` API are removed. The
pre-1.0 extension API changes deliberately; caller migration includes the widget
catalog, runtime/testing consumers and facade. The public gesture helpers serve
external implementations and the built-in recognizers, rather than exposing
unwired utility types.

ADR-0086's write boundary is preserved. `flui-interaction` does not depend on the
reactive graph or accept an `EventCx`; widgets capture their presentation's
`WriterSource` and open a write while invoking their framework callback. The
gesture API change does not widen access to writes during build.

The public `public_recognizer_extension_contracts` table covers overlapping and
invalid admission, frozen settings, cancellation competition, clock reentry,
weak attachments, exact stale-contact retirement, and deferred release of
double-tap and multi-tap holds. `gesture_lifecycle_matrix` and
`arena_settles_every_member_exactly_once` pin verdict delivery and generation
isolation. Widget contract rows
`listener_admission_keeps_terminal_delivery_and_weak_ownership`,
`listener_raw_observer_panic_still_delivers_the_recognizer_event` and
`custom_recognizer_competes_through_a_listener` exercise the mounted consumer.
Compiler fixtures cover dyn compatibility and thread affinity. Performance
comparisons use the saved Criterion workloads described in the
[interaction performance guide](../../crates/flui-interaction/docs/PERFORMANCE.md);
this decision makes no timing claim.
