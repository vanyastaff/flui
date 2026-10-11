# ADR-0191: Animation registries have one parent

- **Status:** Accepted
- **Date:** 2026-10-10
- **Supersedes:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md), shared descendant topology only; [ADR-0125](ADR-0125-vsync-registration-authority.md), child attachment returning `Option` only.
- **Related:** [ADR-0179](ADR-0179-controller-registration-ownership.md), [ADR-0195](ADR-0195-presentation-motion-policy.md).

## Context

A child receives its parent's frame timestamp and motion policy. Two parents
can apply conflicting reduced-motion and mute policies to the same controllers.
Attaching the same child twice under one parent also samples its controllers
twice per frame. Production callers already form a tree: `TickerMode` mirrors
widget ancestry, and the widget harness adopts a registry under its runtime root.

## Decision

A registry has at most one live parent. It stores one optional weak registration
backreference; its parent holds the child strongly. Releasing a parent does not
retain it or prevent later attachment of a surviving child.

`Vsync::attach_child` returns `Result<VsyncRegistration, VsyncRegistrationError>`.
Closed registries, ancestor cycles, an already attached child and permanent
identity exhaustion have distinct refusals. Topology refusal changes no edge and
consumes no identity. Existing backend-and-slot tokens remain the authority for
detachment; foreign and stale tokens cannot remove a replacement attachment.

Cycle preflight walks upward from the proposed parent. Owner-local single-writer
admission needs no descendant snapshot or frame-path cycle stack. Both edge
directions commit before the first frame requester runs, without a borrow across
user code. A failed attachment wake withdraws that edge while preserving its
first failure; existing controllers remain owned and can resume after correction.

Detachment clears the backreference before retiring child custody. Closing a
registry closes its attached descendants and withdraws its parent seat before
cancellation callouts. A refused second parent does not share that closure.

The ultimate live parent identifies a registry's clock authority, including when
an embedder directly ticks an attached subtree. Choosing a different delivery
entry point does not transfer ownership. Each registry remembers its root weakly.
Moving a surviving subtree to another root preserves each controller's
published elapsed time and run future, then anchors that elapsed on its first
delivered frame. Old absolute timestamps and dispatched tick history do not carry
between independent clocks. Detach and reattach under the same root retain the
existing anchors, including catch-up after mute or an interval without delivery.

A frame walk borrows its ancestor admission chain. Before each child or controller
delivery it checks that the captured parent seats still exist and ancestors remain
enabled. Reentrant withdrawal, reattachment or mute stops the affected remainder
of that old walk; unaffected peers remain deliverable. This needs no additional
subtree allocation or strong root custody. Validation costs one admission check
per ancestor; re-anchoring visits resident controllers only when authority changes.

`TickerMode` detaches its old seat before moving under a changed ambient registry.
Refused admission uses its detached scope: finite motion follows missing-clock
settlement instead of waiting indefinitely. Diagnostics run outside borrows.

The widget harness treats re-adopting its current registry as a no-op.
`LaidOut::adopt_vsync` returns a refusal while retaining its current driver.
Initial `lay_out_animated` setup asserts an admissible registry, as other harness
setup assertions do; expected refusals use the fallible replacement operation.

## Alternatives

- A shared graph requiring equal clock identity and policy would introduce
  additional frame authority and still need rules for conflicting ancestor mute
  and closure. No production caller needs it.
- Multiple edges with one authoritative parent would make the other edges
  observation relationships. Combining observation and frame ownership in
  `attach_child` leaves callers responsible for knowing which may tick or close.
- One parent matches existing producers and removes conflicting frame authority
  and duplicate delivery at admission.

## Verification

`vsync_admission_contract` checks policy isolation, duplicate refusal and one
sample, cycles without mutation, reattachment after detach or parent release,
stale tokens, and recovery after a failed attachment wake. Its transfer rows
cover earlier and later clock origins, nested controllers, expired parents and
the original completion future. Direct subtree ticks preserve the same authority
and deadline. Reentrant rows withdraw a later child or an
in-flight ancestor, mute an ancestor, and verify progress under the live driver.
`closing_a_registry_withdraws_kernels_before_delivery` retains nested cancellation,
first-failure custody and independent presentation progress. Private terminal
capacity rows continue pinning permanent exhaustion.

`widget_registry_adoption_preserves_its_driver_on_refusal` checks same-registry
adoption, foreign and closed refusal, retained-driver progress and later successful
replacement. `a_detached_ticker_mode_lands_an_implicit_run` and
`reparenting_across_ticker_mode_moves_the_registration` cover absent/refused ambient
clocks and moving mounted owners between real ticker modes.

## Compatibility

Child callers handle typed `Err` instead of `None`. Explicit child sharing is
replaced by distinct identities under the respective presentation roots.
Controller admission stays private and owned by `DrivenController`; direct virtual
frame driving remains an explicit embedder operation.
