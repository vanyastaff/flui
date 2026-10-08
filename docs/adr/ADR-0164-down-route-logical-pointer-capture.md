# ADR-0164: Down-route logical pointer capture

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)
  (owned contact identity and terminal events),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)
  (exceptional retirement).

## Context

The binding retains an admitted Down route until its contact ends. Ordinary
delivery visits the whole retained route, even when motion leaves the original
hit area. An actual Listener may need exclusive later delivery while retaining
the initial Down observation round. That authority must belong to the exact
contact and presentation, without keeping either alive or permitting a stale
native pointer ID to cancel a replacement contact.

## Decision

### Authority and routing

`PointerDispatch::capture()` in an admitted target's Down callback returns a
non-Clone, owner-affine `PointerCapture` or a matchable `PointerCaptureError`.
Synthetic dispatch has no authority; another event kind cannot acquire it.
The first successful claimant wins. Every target in the original committed
Down route still observes that Down; subsequent contact packets select the
claimant's retained target and transform. Without a claim, delivery retains the
whole implicit Down route. Pointer-router and gesture-arena lifecycle remain
independent of this target selection.

The contact owns capture state carrying its existing binding generation and
actual `PointerInfo`. The token contains only a weak reference to that state,
with the original identity, generation and claimant. It cannot keep a
presentation alive, act on a sibling owner, or retire a new contact after native
ID reuse. Native termination, replacement Down and owner teardown invalidate
release authority before callbacks. Selection remains available for the
generation's already accepted motion and terminal cleanup.

### Release, delivery debt and recovery

Drop and consuming `release()` commit one capture-loss obligation before waking
the owning presentation. They invoke no event handler. The wake uses that
presentation's weak `PlatformWindow`, installed through the runtime's hidden
binding bridge; there is no primary-window fallback. Internal state is committed
and all borrows are released before calling `request_redraw` or retiring the
upgraded window owner. A platform implementer can reenter or panic from that
call. Existing first-failure containment and ADR-0127 govern the wake and its
retirement: ordinary failure propagates after debt is committed, while an active
unwind keeps its earlier failure authoritative. Failed waking never erases debt.

The binding settles release debt at its next input or frame entry. It checks the
exact generation, detaches it, delivers already accepted queued or resampled
motion over the frozen selected route, emits one `Cancel(CaptureLost)`, and
finishes arena, route and sampling cleanup before propagating the first failure.
A committed frame batch remains deliverable if another callback releases its
token; the batch finishes before loss settlement. Release alone cannot discard
accepted motion. A native terminal or replacement generation retains its own
existing admission and stale-work policy.

New old-contact motion after release is refused until a fresh Down, rather than
being reinterpreted as hover after route removal. The existing bounded refusal
slots distinguish release from capacity refusal, which waits for native
termination. A tracked released tail matches pointer ID and optional device ID;
another identified device with the same native pointer ID can still deliver
hover. When device identity is absent, two unreported sources cannot be
distinguished. A new physical contact omitted by the producer cannot be inferred
from a bare Move: Down is the observable admission boundary. Refusal storage
retains its conservative saturation policy, suppressing untracked tails until a
lifecycle reset instead of allocating unbounded state.

### Unmount and owner close

Unmounting an ordinary Listener withdraws its target from future hit tests. It
does not close the presentation's dispatch owner or erase an admitted route's
retained handler cell. That contact still owes terminal cleanup. Releasing its
retained token after unmount therefore arranges one deferred `CaptureLost` on
the cached route, with no event callback inside Drop and no later released Move
or duplicate terminal. Presentation close is a separate boundary: it consumes
or invalidates obligations according to the existing ordinary or preserving
close policy. Tokens are inert after their exact contact state retires.

### Native capability boundary

This token controls logical target routing. Existing platform automatic capture
and native `Cancel(CaptureLost)` production keep their backend contracts.
Dropping or releasing the logical token does not perform or advertise an
explicit OS capture-release operation. Native hardware delivery is not implied
by the logical contract tests.

## Validation

`explicit_pointer_capture_contract` exercises the public interaction lane and
binding: full Down observation, exclusive later routing, implicit routing,
typed acquisition failures, stale generation, owner/device isolation, native
termination, accepted queued and committed-frame motion, wake failure,
reentrant release, competing callback failures and subsequent healthy admission.

The actual widget rows
`listener_capture_retains_one_target_and_drop_delivers_loss` and
`listener_unmount_preserves_one_captured_contact_terminal` exercise Listener
retention, nested claim order, deferred cancellation and replacement-tree
admission. They run in `contracts::pointer_and_gesture_recognition`.
Independent removal of target filtering, release debt, the in-flight guard or
device matching makes the corresponding public behavior assertions fail;
an API-absence compiler failure alone does not establish this behavior.

## Consequences

Contact state and a weak token encode authority without another identity
allocator or callback registry. The runtime supplies the actual presentation
wake, and Listener uses its existing Down callback rather than a second capture
notification API. `PointerDispatch::new` and `at_root` remain valid synthetic
borrowed dispatch constructors without capture authority. Acquiring a token
does not alter the already committed Down round or gesture arbitration.
