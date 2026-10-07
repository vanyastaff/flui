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

The public wire uses `flui-platform-api`'s owned `PointerEvent` and `KeyEvent`
contracts. `PointerId` names a contact, `DeviceId` names hardware, and
`PointerInfo` carries the reported `PointerKind` and primary role. Samples use
checked logical positions and monotonic `EventTime`. Missing sensors stay
absent; a backend must not substitute pressure, orientation or device identity
that the host did not report. Private backend adapters may still translate an
upstream event vocabulary; richer native production is a separate migration.

## Gesture recognition

Configure callbacks before sharing the recognizer. `build()` returns an
`Rc`; callbacks cannot be replaced after construction and may capture owner-local
UI state.

```rust
use std::{cell::Cell, rc::Rc};
use flui_interaction::{GestureArena, TapGestureRecognizer};

let taps = Rc::new(Cell::new(0));
let observed = taps.clone();
let tap = TapGestureRecognizer::builder(GestureArena::new())
    .on_tap(move |_| observed.set(observed.get() + 1))
    .build();

// Keep `tap` in widget state for as long as it should receive input.
```

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
`GestureDetector` keeps its familiar `on_tap`, `on_pan_*` and other callback
API. Explicit `cancel()` delivers cancellation and leaves a recognizer reusable.
Dropping the last owner releases membership silently, without invoking gesture
callbacks. See [GESTURES.md](docs/GESTURES.md) and the
[custom recognizer example](examples/custom_recognizer.rs).

## Focus

One presentation owns one manager. Retain the attachment while the node is
mounted:

```rust
use flui_interaction::{FocusManager, FocusNode};

let manager = FocusManager::new();
let node = FocusNode::new();
let _attachment = manager.root_scope().attach_node(&node)
    .expect("a fresh node attaches to its presentation root");
node.request_focus();
assert!(node.has_primary_focus());
```

Reentrant focus requests are admitted to a synchronous FIFO. Committed observer
rounds and accepted requests complete before the first observer failure resumes.
Focus closure commits terminal ownership before retiring callbacks and contexts.

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

Processing includes bounded velocity estimation, frame resampling, prediction
and pointer smoothing. Recognizers use event timing anchored to their arena
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
