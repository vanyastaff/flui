# flui_interaction

Owner-local pointer routing, hit testing, focus management and gesture recognition
for FLUI. The synchronous input path belongs to a presentation; it has no
process-global focus or gesture owner.

## Event flow

```text
Platform pointer events
    → GestureBinding
    → hit test and InteractionLane route
    → recognizers
    → GestureArena verdicts
    → user callbacks

Keyboard events → presentation FocusManager → focused node's handlers
```

The binding retains the Down route through Up or Cancel. It routes the whole
Down before closing the arena, sweeps after Up, and does not force a winner
on Cancel. Each target receives `PointerDispatch`: `local` is transformed for
that target, while `global` preserves the original event.

An actual target's Down callback can retain a successful `dispatch.capture()` as a
non-Clone `PointerCapture`. The first claim selects that target for later
contact packets; the original Down still visits the entire route. Drop or
consuming `release()` schedules one `CaptureLost` after accepted motion, without
calling an event handler inside release. The token weakly references its exact
contact generation, so an ended contact or closed presentation makes it inert.
This is logical routing capture; it does not request explicit OS release.
See [ADR-0164](../../docs/adr/ADR-0164-down-route-logical-pointer-capture.md).

The public wire uses `flui-platform`'s owned `PointerEvent` and `KeyEvent`
contracts. `PointerId` names a contact, `DeviceId` names hardware, and
`PointerInfo` carries the reported `PointerKind` and primary role. Samples use
checked logical positions and monotonic `EventTime`. Missing sensors stay
absent; a backend must not substitute pressure, orientation or device identity
that the host did not report. Backends produce this vocabulary at the platform
boundary rather than exposing their private host event types.

## Gesture recognition

Configure callbacks before sharing the recognizer. `build()` returns an
`Rc`; callbacks cannot be replaced after construction and may capture owner-local
UI state.

```rust
use std::{cell::Cell, rc::Rc};
use flui_foundation::geometry::Point;
use flui_platform::EventTime;
use flui_interaction::{
    GestureBinding, HitTestEntry, HitTestResult, InteractionLane, PointerButton,
    PointerButtons, PointerEvent,
    PointerId, PointerInfo, PointerKind, PointerPosition, PointerPress,
    PointerRelease, PointerSample, RecognizerSet, RenderId, TapGestureRecognizer,
};

let lane = InteractionLane::try_new()?;
let binding = GestureBinding::new();
let taps = Rc::new(Cell::new(0));
let observed = taps.clone();
let tap = TapGestureRecognizer::builder(binding.arena().clone())
    .on_tap(move |_| observed.set(observed.get() + 1))
    .build();
let mut recognizers = RecognizerSet::default();
recognizers.attach(&tap); // `tap` remains the strong owner.
let pointer = PointerId::new(std::num::NonZeroU64::MIN);
let info = PointerInfo::new(pointer, PointerKind::Touch);
let position = PointerPosition::try_new(Point::new(12.0, 20.0))?;
let down = PointerEvent::Down(PointerPress::new(
    info, PointerButton::PRIMARY, PointerButtons::NONE,
    PointerSample::new(EventTime::from_nanos(0), position),
));
let up = PointerEvent::Up(PointerRelease::new(
    info, PointerButton::PRIMARY, PointerButtons::only(PointerButton::PRIMARY),
    PointerSample::new(EventTime::from_nanos(20_000_000), position),
));
lane.enter(|| -> Result<(), Box<dyn std::error::Error>> {
    let target = lane.dispatch_handle().register_pointer(move |dispatch| {
        recognizers.dispatch(dispatch);
    })?;
    let route = |_| {
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        path
    };
    binding.handle_pointer_event(&down, route);
    binding.handle_pointer_event(&up, route);
    Ok(())
})?;
assert_eq!(taps.get(), 1);
# Ok::<(), Box<dyn std::error::Error>>(())
```

This standalone composition uses the same `GestureBinding` as a presentation:
the binding owns exact-contact Down closure, Up sweeping, Cancel retirement and
deferred resolution draining after the full hit route. A host still drives idle
progress: schedule `binding.next_deadline()`, call `binding.tick_deadlines()` and
`binding.drain_deferred_arena_resolutions()` when due, and flush pending motion
on frames. Keep these calls inside the active lane even without another pointer
packet. The custom recognizer example demonstrates both routed contact delivery
and a stationary long-press deadline using public input constructors.

Mounted widgets acquire the presentation's shared, binding-driven arena through
`flui_widgets::GestureArenaScope::of(ctx)` in their lifecycle setup, retain
recognizers in state, and attach them with `Listener::recognizer`.
`GestureBinding` owns routing, closure, sweeping and draining there; the
presentation frame loop drives its idle deadlines and pending motion.
`GestureArena::new()` creates a private self-driven arena for one recognizer
owner; it cannot back `GestureArenaScope`. A private owner closes Down after
admission, polls deadlines and drains deferred resolutions; built-in
recognizers finish their exact admitted membership on Up or Cancel. Adding an
ID-only sweep after a callback can instead sweep a newly admitted same-ID
contact, so composed input uses the binding's complete route transaction.

Tap Down callbacks receive `TapDownDetails`; Up and completed-tap callbacks
receive `TapUpDetails`. Move and cancellation callbacks receive `TapDetails`.
Long-press movement receives `LongPressMoveUpdateDetails` with local/root
displacements from Down; its terminal callback receives `LongPressEndDetails`
with measured local velocity. These names are exported at the crate root and
through `flui::interaction`.

`GestureRecognizer` is an open, dyn-compatible extension point with
`add_pointer(&self, PointerDispatch<'_>)`, `handle_event(&self,
PointerDispatch<'_>)` and reusable `cancel(&self)`. `GestureArenaMember`
provides arbitration and optional deadline methods. The arena and recognizer
attachments hold weak references, so a cached route cannot extend a
recognizer's lifetime.

`RecognizerSet` is the shared attachment mechanism for listeners. Admission
filters apply only to Down; already admitted contacts still receive their
terminal events. Keep strong recognizer ownership beside the attachment:

```rust
use flui_interaction::{GestureArena, RecognizerSet, TapGestureRecognizer};

let tap = TapGestureRecognizer::builder(GestureArena::new()).build();
let mut attachments = RecognizerSet::default();
attachments.attach(&tap);
assert!(!attachments.is_empty());
```

In widgets, `Listener::recognizer` and `recognizer_when` use this mechanism.
`GestureDetector` mounts its typed slots (`on_tap`, `drag(DragSlot)`,
`transform(TransformSlot)` and the rest) through `RawGestureDetector`. Explicit `cancel()` delivers cancellation and leaves a recognizer reusable.
Dropping the last owner releases membership silently, without invoking gesture
callbacks. See [GESTURES.md](docs/GESTURES.md).

## Focus

One presentation owns one manager. Retain the attachment while the node is
mounted:

```rust
use flui_interaction::{FocusManager, FocusNode};

let manager = FocusManager::new();
let node = FocusNode::new();
let _attachment = manager.root_scope().attach_node(&node)
    .expect("a fresh node attaches to its presentation root");
let _ = node.request_focus();
assert!(node.has_primary_focus());
```

Reentrant focus requests are admitted to a synchronous FIFO. Committed observer
rounds and accepted requests complete before the first observer failure resumes.
Focus closure commits terminal ownership before retiring callbacks and contexts.

For a shortcut whose lifetime follows a widget or plugin, retain its scoped
registration. Dropping it withdraws only that registration; the token does not
keep the manager alive. Presentation-wide handlers can instead use the explicit
`add_global_key_handler` lifetime.

```rust
use std::rc::Rc;
use flui_interaction::{FocusManager, KeyEventResult};

let manager = FocusManager::new();
let shortcut = manager.register_global_key_handler(Rc::new(|_event| {
    // Match the desired key here before returning Handled.
    KeyEventResult::Ignored
})).expect("the presentation is open and identity is available");
drop(shortcut); // No sibling registration is removed.
```

## Hit testing and cursors

`HitTestResult` records data-only target identities and global-to-local
transforms. Scoped paint-offset and paint-transform helpers install inverses
and restore their entry depth, including after a caught panic. A non-finite or
non-invertible paint transform refuses traversal.

`HitTestBehavior::{Opaque, Translucent, DeferToChild}` controls contribution and
occlusion. Ordinary pointer delivery visits every hit target leaf-first;
scroll and pan-zoom walks allow a handler to claim an event.
`CursorRequest::Defer` leaves cursor selection to the next target, whereas
`Icon(CursorIcon::Default)` explicitly requests the arrow.
See [HIT_TESTING.md](docs/HIT_TESTING.md).

## Settings and processing

Gesture settings reject invalid numeric ranges. Each contact freezes settings
at admission, so the meaning of a gesture does not change midway through it.

```rust
use std::time::Duration;
use flui_interaction::{GestureArena, GestureSettings, TapGestureRecognizer};

let settings = GestureSettings::default()
    .with_double_tap_timeout(Duration::from_millis(300))
    .try_with_touch_slop(18.0)
    .expect("finite nonnegative slop");
let tap = TapGestureRecognizer::builder(GestureArena::new())
    .settings(settings)
    .build();
```

Processing includes bounded velocity estimation, frame resampling and One Euro
pointer smoothing. Hardware predictions remain separate source samples.
`GestureSettings::with_position_smoothing` makes drag, multi-drag, tap-and-drag,
long-press, force-press and scale smooth the positions they publish after
recognition (ends, velocities and native pan-zoom stay raw); slop, arena and velocity decisions
keep the raw samples, and each accepted gesture starts unsmoothed. No preset
selects smoothing. `OneEuroTuning::new` validates parameters with a typed
error; filtering rejects invalid samples the same way. Invalid input leaves
accepted history unchanged; the 2D filter commits both axes together, and a
duplicate or regressed timestamp holds its accepted result and clock anchor.
Interpolating a position keeps the preceding measured
sensor reading, including missing pressure; it does not measure a new pressure.
Recognizers use event timing anchored to their arena
clock; replayed sample spacing determines the gesture's velocity.
Gesture scripts and virtual-time replay live in `flui-testing`; the `testing`
feature here supplies individual synthetic input builders.

Down, Move and Up fixture helpers return a checked `Result`; handle admission
errors rather than publishing non-finite positions. Coalesced measured samples
and predicted samples remain distinct. Velocity uses actual measurement
history, while predictions travel as separate source data.

`Listener` callbacks in `flui-widgets` receive `PointerDispatch` over this
same owned event: `local` carries localized positions and sample histories,
while `global` preserves the original source. Sensor absence remains `None`;
callbacks do not need a second raw-event adapter or input-mode switch.

## Ownership and threading

Recognizers, their builders, arena, focus callbacks and executable pointer
attachments are `!Send + !Sync`. Dispatch stays synchronous on their UI owner.
Events, route identities and hit-test data retain the thread-safety needed by
the rendering and embedding boundaries. Callback invocation and capture
retirement happen outside internal borrows. Containment preserves the first
failure and accepted delivery according to the subsystem's contract.

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for ownership decisions and named
contract tests, and [PERFORMANCE.md](docs/PERFORMANCE.md) for cost bounds and
benchmark commands.
