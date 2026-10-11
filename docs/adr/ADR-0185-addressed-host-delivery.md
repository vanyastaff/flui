# ADR-0185: Host delivery preserves runtime and presentation ownership

- **Status:** Accepted
- **Date:** 2026-10-11
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md), [ADR-0039](ADR-0039-event-loop-affinity-capability.md), [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md), [ADR-0182](ADR-0182-complete-owner-execution.md).

## Decision

Each UI runtime's host wake owns separate redraw debt and a weak reference to
its window. Installing another runtime cannot redirect that wake, and rendering
a sibling cannot clear its debt. Loop-wide infrastructure notifications remain
loop-wide; the next owner turn distributes changes to their runtime consumers.

The desktop platform installs one deadline hook per owner. Its answer folds
logical runtime deadlines, native text-sizing deadlines and each live
presentation's recovery, fallback and first-reveal deadlines. Presentation
metadata remains available while its native driver is leased. Retirement removes
that presentation's contribution without replacing the hook or affecting siblings.
Visual deadlines obey frame eligibility; native text-sizing deadlines retain
their independent progress contract.

Service startup separates preparation, user factory invocation, registry commit
and executor invocation. Only preparation and commit borrow app storage. Commit
checks the original owner generation and current admission before publishing the
service. Refused futures retire outside the app borrow. The completion channel
is registered before the executor can synchronously run accepted work.

`WindowRequest` observes installation of either a shown main window or a
content-bearing secondary window. The immediate `Result` from `open_window`
describes admission and native creation; the receipt settles after publication
and initialization, including renderer, mount and installer failure. Dropping the
observer does not cancel accepted installation. Success identifies an installed
presentation; it promises neither compositor presentation nor native focus.

The runtime owner's callback containment boundary emits a typed failure report
for the affected address before returning its runtime lease. This report does not
change frame-failure streaks or replace the original panic. The handler runs
outside owner-storage borrows and retains the existing containment policy.

Lifecycle and timing notifications participate in the scheduler's failure
custody even when delivered outside a frame. Nested owner execution inherits
that receipt and restores its enclosing binding. Catching an observer panic
therefore cannot permit subsequent cancellation to destroy opaque captures
while the first failure is still being preserved.

## Alternatives considered

Per-window deadline hooks and a shared redraw flag require installation order
and callers to preserve sibling obligations. One owner hook and addressed
runtime wakes remove those responsibilities. Invoking a service factory while
borrowing the registry prevents legitimate reentry; making that registry
reentrant instead would add shared mutable ownership to an owner-local path.
Prepare/construct/commit preserves exclusive storage access and checks owner
replacement before accepting the future. Returning only native-window admission
cannot describe deferred renderer or initialization failure, so installation has
an explicit receipt.

## Evidence and limits

`host_delivery_contract` exercises two headless windows, independent redraw debt,
service factory reentry, owner replacement during factory invocation and
unsupported storage admission. `installed_frame_driver_contract` checks deadline
folding while an installed native driver is leased, eligibility changes and
sibling retirement.
`secondary_installation_contract` observes successful initialization, early close
and initialization panic through the receipt. It also checks typed renderer and
installer failure, cancellation after owner replacement, and shutdown of multiple
pending windows when a receiver's waker panics. `owner_delivery_contract` observes
typed failure delivery and preserved cross-runtime work after containment.

These headless and injected-driver cases establish host bookkeeping, not native
OS event conversion, a GPU presentation or mobile background execution.
