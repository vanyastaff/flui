# Gesture Recognition Guide

How the recognizers in `flui_interaction::recognizers` behave. Every position,
delta, scale, pressure and velocity field is `f64` (`Offset<f64>` for points);
times are `Instant`/`Duration`.

## Traits

Both extension traits are open and dyn-compatible. `GestureArenaMember`
provides `accept_gesture`, `reject_gesture`, `deadline() -> Option<Instant>`
and `poll_deadline(now)`. The arena reads its clock outside state borrows and
polls only armed, due deadlines.

`GestureRecognizer: GestureArenaMember` adds `add_pointer(&self,
PointerDispatch<'_>)`, `handle_event(&self, PointerDispatch<'_>)` and
`cancel(&self) -> CancelOutcome`. A heterogeneous collection can store
`Rc<dyn GestureRecognizer>` and upcast to the arena-member trait.

Built-in recognizers: `TapGestureRecognizer`, `DoubleTapGestureRecognizer`,
`LongPressGestureRecognizer`, `DragGestureRecognizer` (plus the
`vertical_drag`, `horizontal_drag` and `pan` constructors in `drag_variants`),
`ScaleGestureRecognizer`, `ForcePressGestureRecognizer`,
`MultiTapGestureRecognizer`, `MultiDragGestureRecognizer`,
`EagerGestureRecognizer` and `TapAndDragGestureRecognizer`.

## Construction

`builder(arena)` returns an unshared builder. Drag additionally takes its axis,
multi-tap its contact count, and multi-drag its axis. Configure callbacks and
`.settings(GestureSettings)` before `.build()`, which returns `Rc<Self>`.
Callbacks are immutable after construction and may capture `Rc` UI state.

```rust
use flui_interaction::{GestureArena, TapGestureRecognizer};

let tap = TapGestureRecognizer::builder(GestureArena::new())
    .on_tap_down(|_| { /* contact */ })
    .on_tap_up(|_| { /* release, after the arena win */ })
    .on_tap(|_| { /* tap */ })
    .on_tap_cancel(|_| { /* cancelled */ })
    .build();
```

Tap also has `on_secondary_tap*` and `on_tertiary_tap*` per button
(`TapButton`).

## Recognizers

The descriptions below state observable recognition behavior; phase names are
only a shorthand for the corresponding state machine.

### Tap (`tap.rs`, `TapState`)

`Ready → Down` on pointer down. Up within slop fires `on_tap_up` and `on_tap`
once the recognizer wins the arena (the release is held in `pending_up` until
`accept_gesture`). Moving beyond the kind-specific slop goes to `Cancelled`.
`TapDetails { global_position, local_position, kind }`.

### Double tap (`double_tap.rs`, `DoubleTapPhase`)

Phases: `Ready`, `FirstDown`, `WaitingForSecond`, `SecondDown`, `Completed`,
`Cancelled`. The first up holds the first arena entry and waits.

- A second down within `double_tap_timeout()` and within `double_tap_slop()` of
  the first goes to `SecondDown`.
- A second down farther than `double_tap_slop()` is ignored: the first entry
  stays held and the phase stays `WaitingForSecond`.
- After the window expires, deadline polling releases the held entry (so a
  competing single tap can win), fires `on_double_tap_cancel`, and returns to
  `Ready`; a new contact then counts as a first tap.
- Movement beyond slop during `FirstDown` or `SecondDown` cancels.

Builder methods: `on_double_tap`, `on_double_tap_down`,
`on_double_tap_cancel`.

### Long press (`long_press.rs`, `LongPressPhase`)

`Ready → Possible` on down; `Possible → Started` when `long_press_timeout()`
elapses within slop; movement beyond slop before that cancels. Firing goes
through one deadline path, reached from frame polling or subsequent input.
It accepts the arena
entry before invoking `on_long_press` / `on_long_press_start`, so a competing
tap on the same region is already rejected when the callbacks run.

Builder methods: `on_long_press_down`, `on_long_press`,
`on_long_press_start`, `on_long_press_move_update`,
`on_long_press_up`, `on_long_press_end`, `on_long_press_cancel`.

### Drag (`drag.rs`, `DragPhase`)

`Ready → Possible` on down. The drag starts (`Started`) when the recognizer
wins its pointer's arena; moving past `pan_slop_for(kind)` along the axis
claims the win while competitors remain. `DragStartBehavior::Start` (default)
reports the position at acceptance, `Down` the down position. Up or an accepted
Cancel ends the drag; rejection before acceptance fires `on_cancel`.

```text
pub enum DragAxis { Vertical, Horizontal, Free }

pub struct DragDownDetails   { global_position, local_position, kind }
pub struct DragStartDetails  { global_position, local_position, kind, timestamp: Instant }
pub struct DragUpdateDetails { global_position, local_position, delta, primary_delta: f64, kind }
pub struct DragEndDetails    { reason: GestureEndReason, velocity: Velocity,
                               global_position, local_position, primary_velocity: f64 }
```

`delta` and `primary_delta` are per update, not cumulative.
`DragEndDetails::reason` is `Completed` for pointer Up and `Cancelled` for an
accepted pointer Cancel; the measured velocity is kept in both cases
(ADR-0112). `DragGestureRecognizer::is_fling(&velocity)` compares speed with
`min_fling_velocity()`. Completed Up evaluates velocity at the terminal event's
time, so a stationary pause before release cannot publish an old fast fling.

### Scale (`scale.rs`, `ScalePhase`)

`Ready → Possible` when a second pointer goes down, which captures the
baseline (span, per-axis spans, focal point, rotation). `Possible → Started`
when any one of these holds (`should_accept`):

- `|span − initial_span| > span_slop_for(kind)`;
- the ratio `span / initial_span` crosses `scale_slop()` (5% by default);
- the focal point moved more than `pan_slop_for(kind)` (two-finger pan).

Calculations (`calculate_spans`, `calculate_rotation`):

- focal point: mean of the active pointer positions;
- span: mean distance from each pointer to the focal point (half the pointer
  distance for two pointers); horizontal and vertical spans use `|dx|` and `|dy|`;
- `scale = span / initial_span`, likewise per axis;
- rotation: the line angle between the two pointers (the mean angle about the
  focal point for more), minus the baseline angle.

Dropping below two pointers ends a started gesture. `ScaleEndDetails` carries
`focal_point`, `scale`, `rotation` and `velocity` (scale units per second).

The default `ScaleStartMode::Scale` requires two contacts. Choose
`ScaleStartMode::PanOrScale` explicitly for one-contact pan that continues as
contacts join or leave:

```rust
use flui_interaction::{GestureArena, ScaleGestureRecognizer};
use flui_interaction::recognizers::scale::ScaleStartMode;

let recognizer = ScaleGestureRecognizer::builder(GestureArena::new())
    .start_mode(ScaleStartMode::PanOrScale)
    .on_update(|details| {
        let local_motion = details.focal_point_delta;
        let global_focal = details.focal_point;
    })
    .build();
```

### Force press (`force_press.rs`, `ForcePressPhase`)

Mouse pressure and constant synthetic readings do not establish a pressure
sensor. A non-mouse contact must supply varying nonzero readings before force
recognition starts; non-finite readings are ignored. The recognizer starts at `start_pressure`
(`FORCE_PRESS_START_PRESSURE = 0.4`) and `Peaked` at `peak_pressure`
(`FORCE_PRESS_PEAK_PRESSURE = 0.85`); both are configurable with
`start_pressure` / `peak_pressure` on the builder. Dropping below the start pressure
from an active press, pointer Up, or movement beyond `hit_slop(kind)` ends a started
press; movement beyond slop while `Possible` rejects it silently.
`ForcePressDetails { global_position, local_position, pressure, max_pressure }`.

### Multi-tap (`multi_tap.rs`, `MultiTapPhase`)

`builder(arena, n)` recognizes `n` simultaneous contacts that stay within slop and
are all released. Each contact is tracked by its own pointer identity
(`multi_contact_events_keep_independent_pointer_identity`).

### Trackpad pan-zoom (`pan_zoom.rs`)

`PointerEvent::PanZoom` carries the canonical `PanZoomEvent`. Its phase is
`Start`, `Update(PanZoomTransform)`, `End` or `Cancelled`. Pan offset, scale
and rotation in an Update are cumulative since Start; consumers subtract pan
and rotation or divide scales to derive one step. `PanZoomEvent` always reports
`PointerKind::Trackpad` and keeps its pointer identity, production time,
logical focal position and modifiers.

Native claims receive `PanZoomDispatch`: `local` contains checked receiving-plane
geometry and `global` retains the original source event. Local cumulative pan
uses a chord at the Update's current focal; scale and rotation remain unchanged
(see [HIT_TESTING.md](HIT_TESTING.md)). `ScaleGestureRecognizer::handle_pan_zoom`
drives the same immutable callbacks used for touch input.

`GestureDetector` admits a native source when enabled scale callbacks accept a
meaningful Update. `GestureBinding` retains the selected owner until the source
ends or is cancelled; an enabled descendant cannot steal that session during
a rebuild. `InteractiveViewer` consumes these callbacks rather than retaining
a second native actor. Public rows
`viewer_native_session_reports_one_start_and_one_terminal`,
`viewer_repeated_native_start_retires_the_previous_generation` and
`viewer_native_owner_survives_descendant_enable_during_rebuild` pin this lifecycle.
An Update without Start is an independent compatibility step: the recognizer
emits start/update/end for that step instead of inventing a continuing source.

## GestureSettings (`settings.rs`)

| Accessor | Default (touch) |
|---|---|
| `touch_slop()` | 18.0 (`hit_slop(kind)` gives a mouse 1.0) |
| `pan_slop()` | 18.0 (`pan_slop_for(kind)` is per kind) |
| `scale_slop()` | 0.05 (a ratio) |
| `double_tap_slop()` | 100.0 |
| `double_tap_timeout()` | 300 ms |
| `long_press_timeout()` | 500 ms |
| `min_fling_velocity()` | 50.0 px/s |
| `max_fling_velocity()` | 8000.0 px/s |

Presets: `touch_defaults`, `mouse_defaults`, `pen_defaults`, `android_defaults`
(400 ms long press), `ios_defaults`, `for_device`, `for_platform`, `native`.
Timing builders include `with_double_tap_timeout` and `with_long_press_timeout`.
Numeric builders such as `try_with_touch_slop`, `try_with_pan_slop` and
`try_with_fling_velocity` return `GestureSettingsError` for invalid ranges.
Each admitted contact freezes a settings snapshot for its complete sequence.

## Arena integration

A recognizer joins the arena from `add_pointer(dispatch)`. `ArenaMembership`
holds the exact allocation's weak identity, established with `Rc::new_cyclic`.
`PrimaryContact` combines membership with one admitted contact, its distinct
`ContactId`, frozen settings, deadline and slop checks. `begin` refuses an
already active contact rather than replacing it.
`GestureArena::add` returns a `GestureArenaEntry` whose `resolve`, `hold`,
`release` and `sweep` act on that one membership. The arena calls
`accept_gesture` / `reject_gesture` after releasing its own borrows.

Arena members, eager winners and pending notifications are weak. A callback
upgrades its participant only immediately before invocation; an owner released
by an earlier callback is skipped. Silent contact destruction queues exact
membership withdrawal for deferred resolution and never calls peers inline.

At runtime `GestureBinding` hit-tests on Down, calls `add_pointer` along the
route, closes the arena, sweeps on Up, and does not sweep on Cancel.

## Velocity tracking (`processing/velocity.rs`)

```rust
use web_time::Instant;
use flui_foundation::geometry::Offset;
use flui_interaction::{PointerKind, Velocity, VelocityTracker};

let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
let now = Instant::now();
tracker.add_position(now, Offset::ZERO);
let v: Velocity = tracker.velocity_at(now); // pixels_per_second: Offset<f64>
let estimate = tracker.estimate_at(now); // Option<VelocityEstimate>
```

A quadratic least-squares fit over at most 20 samples within a 100 ms horizon;
fewer than 3 samples gives no fit, and a pointer still for 40 ms reports zero.
Use `processing::VelocityTracker::with_estimator(kind, estimator)` for a
different `processing::VelocityEstimator`, or configure a recognizer through
`GestureSettings::with_velocity_estimator`. Drag, multi-drag, tap-and-drag and
scale capture this policy when their contact sequence begins. Defaults remain
least squares on every platform; selecting weighted recent intervals or impulse
integration is an authored policy rather than an OS preference. All algorithms
share the explicit sample-clock stop gate exposed by `estimate_at(now)`.

Owned `PointerMove` exposes current, coalesced and predicted samples separately.
Recognizers consume measured history in source order and never feed predictions
into velocity estimation. Device timestamps, including epoch zero, remain
valid `EventTime` values. Button changes are mid-contact state, not a second
Down; device-only events do not invent a pointer contact.

## Custom recognizers

Implement `GestureArenaMember` and `GestureRecognizer` directly. Construct the
recognizer with `Rc::new_cyclic`, pass its weak identity into
`ArenaMembership::new`, and use `PrimaryContact` when recognition tracks one
contact. Multi-contact recognizers use membership for each independent pointer.
Measure with `dispatch.local` and report root coordinates from `dispatch.global`.

Keep strong ownership in widget state, and attach through
`Listener::recognizer` or `RecognizerSet`. Attachments hold weak identities.
`recognizer_when` / `attach_when` filter new Down admission only; they cannot
erase the Move/Up/Cancel tail of a contact admitted earlier. The
[custom recognizer example](../examples/custom_recognizer.rs) demonstrates the
open extension point without a parallel marker trait.

Explicit `cancel()` delivers cancellation once for an active sequence and
returns `Cancelled`, or `Idle` when there is none. It does not disable the
recognizer. `cancel_all` attempts every supplied recognizer before resuming the
first failure. Last-owner destruction withdraws contacts silently and retires
captures outside state borrows, preserving an incoming or previously caught
failure according to ADR-0127.

## Ownership and threading

Recognizers, the arena and their callbacks are owner-local (ADR-0027):
callbacks are `Rc<dyn Fn>` and may capture `Rc<Cell<_>>` state. Pointer
events, IDs and settings stay `Send + Sync` where they cross runtime
boundaries.

## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [HIT_TESTING.md](HIT_TESTING.md)
- [PERFORMANCE.md](PERFORMANCE.md)
