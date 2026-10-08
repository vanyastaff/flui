# ADR-0170: Presentation-owned consumptive signal routes

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)
  (owned source identity and native phases),
  [ADR-0164](ADR-0164-down-route-logical-pointer-capture.md)
  (independent contact routing),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)
  (first-failure authority and retirement).

## Context

Nested Scrollables and InteractiveViewers arbitrate a consumptive signal by
returning `EventPropagation::Stop`. Repeating that arbitration against each
packet's current hit path lets a newly enabled descendant steal an admitted
gesture, or transfers a wheel burst to an ancestor when its selected scroller
reaches an extent. Raw pointer observation still needs current geometry.
Consumption and observation therefore require distinct routing lifetimes.

The presentation already owns its interaction binding, lane targets and
monotonic clock. Selection belongs there, without another public event channel,
platform registry or process-wide state.

## Decision

### Observation and selection

Every scroll and native pan/zoom packet hit-tests current geometry for raw
pointer observers. The independent consumptive walk selects the first handler
that returns Stop. The binding retains its typed lane target and admitted
global-to-local transform for that source sequence. Later packets visit that
exact target, even after hierarchy changes or an extent is reached. An
unregistered target is inert until the sequence ends; the binding does not
chain the remainder to another consumer.

Render hit entries supply existing `ScrollTarget` and `PanZoomTarget`
identities. Scrollable and GestureDetector consume those channels;
InteractiveViewer uses GestureDetector's native gesture delivery. A selection
stores data, not callback ownership. Native delivery preserves the actual
root-space event alongside the selected route's localized event through
`PanZoomDispatch`. Contact Down routing and logical pointer capture remain
independent: a native gesture packet observes fresh geometry even while an
ordinary contact retains its Down route.

### Source and lifetime

Each binding has separate bounded scroll and pan/zoom source maps. A known
`DeviceId` identifies a source; without it, `PointerId` is the fallback. Kind
and role are mutable metadata and cannot split an admitted source. Two sources
with neither distinct device identity nor distinct pointer identity cannot be
distinguished by the consumer.

Native pan/zoom additionally retains its exact gesture `PointerId`: a stale
non-Start packet from another contact cannot terminate or consume the current
device's gesture. Repeated Start for the same source and contact issues a fresh
admission authority while retaining the selected target. The existing
GestureDetector can retire its old recognized generation and stage the new
cumulative session. After terminal withdrawal, a fresh Start may choose a
different target. Producers assign virtual native gesture identities distinct
from live ordinary contacts, as required by the owned event vocabulary.

Scroll Ended, Cancelled and MomentumEnded, and native End and Cancelled,
withdraw authority before terminal observers and consumers run. An unselected
scroll terminal delta still receives one fresh consumptive walk, without
retaining selection. Explicit scroll Begin starts a new selection. Phase-less
wheel bursts expire after 500 ms without a packet on the presentation's
`MonotonicClock`; expiry is checked before the next scroll packet. Native event
timestamps do not control this interval, and expiry needs no asynchronous
timer or redraw. Device removal, focus loss and presentation teardown release
their owned selections.

Each channel admits at most 32 simultaneous sources. Overflow continues fresh
raw observation but refuses new consumption instead of publishing a stream
whose selection cannot be retained.

### Reentry and failure

An `Rc` admission identity distinguishes a current source generation from a
reentrant replacement. Returning Stop publishes selection inside the existing
owner callback containment, before outgoing callback snapshots or captures
retire. A destructor failure after a successful Stop cannot erase the consumed
route. Stale publication and cleanup cannot overwrite replacement admission.

Terminal withdrawal precedes callbacks while the accepted terminal delivery
keeps its old route. A replacement admitted during that round survives its
cleanup. Observer and consumer containment completes accepted work and keeps
the first failure authoritative through competing callback and retirement
failures.

Focus-loss and removed-device contact cancellation batches withdraw their old
signal state before delivery. Later snapshotted contact Cancels bypass a second
signal withdrawal: a signal accepted by an earlier Cancel callback is new work,
even on the same device, and must remain deliverable. Ordinary standalone
Cancel still withdraws its current source before dispatch. Borrows never span
user callbacks or retirement.

## Validation

`binding_input_contract_matrix` exercises selected-target failure and recovery,
fresh observer failure competing with consumer failure, target retirement,
terminal and claim reentry, repeated native Start, capture retirement,
absent-device kind/role changes, and independent focus/device cancellation
batches for both channels.

`contracts::scroll_physics_and_activity` contains actual nested Scrollable rows
for extent retention, changed focal position, independent devices and owner
clock inactivity. The actual widget row
`viewer_native_owner_survives_descendant_enable_during_rebuild`, mounted in
`contracts::pointer_and_gesture_recognition`, verifies that enabling a nested
InteractiveViewer cannot steal its ancestor's selected native source and that
a fresh source can select the descendant after termination.

These are interaction and headless widget contracts. They do not establish
live native event translation or hardware delivery. Acceptance requires
independent removal of selected routing, batch admission preservation and
identity projection to fail the corresponding behavior assertions; compilation
alone is insufficient.

## Consequences

Consumers arbitrate once per admitted source instead of implementing competing
hierarchy ownership policies. Raw observation remains responsive to current
geometry, while consumption has explicit presentation and generation authority.
Reaching an extent or retiring a selected widget intentionally does not hand
the active sequence to an ancestor. A later sequence is the admission boundary
for choosing another consumer.
