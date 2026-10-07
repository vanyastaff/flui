# Gesture Recognition Guide

How the recognizers in `flui_interaction::recognizers` behave. Every position,
delta, scale, pressure and velocity field is `f64` (`Offset<f64>` for points);
times are `Instant`/`Duration`.

## Traits

```
GestureArenaMember (sealed)
 └── GestureRecognizer            add_pointer(self: &Arc<Self>, ..) / handle_event / dispose / primary_pointer
      ├── OneSequenceGestureRecognizer
      └── PrimaryPointerGestureRecognizer   deadline hook (did_exceed_deadline)
```

Built-in recognizers implement `GestureArenaMember` directly. Code outside the
crate implements `CustomGestureRecognizer`, whose blanket impl supplies
`GestureArenaMember` (`src/arena/mod.rs`).

Built-in recognizers: `TapGestureRecognizer`, `DoubleTapGestureRecognizer`,
`LongPressGestureRecognizer`, `DragGestureRecognizer` (plus the
`vertical_drag`, `horizontal_drag` and `pan` constructors in `drag_variants`),
`ScaleGestureRecognizer`, `ForcePressGestureRecognizer`,
`MultiTapGestureRecognizer`, `MultiDragGestureRecognizer`,
`EagerGestureRecognizer` and `TapAndDragGestureRecognizer`.

## Construction

`new(arena)` (drag: `new(arena, axis)`, multi-tap: `new(arena, count)`) and
`with_settings(arena, .., settings)` return `Arc<Self>`. Each `with_on_*`
builder takes and returns that `Arc`. `set_settings(&self, settings)` replaces
the settings later. Callbacks are stored as owner-local `Rc<dyn Fn>`.

```rust
let tap = TapGestureRecognizer::new(arena.clone())
    .with_on_tap_down(|d: TapDetails| { /* contact */ })
    .with_on_tap_up(|d| { /* release, after the arena win */ })
    .with_on_tap(|d| { /* tap */ })
    .with_on_tap_cancel(|d| { /* cancelled */ });
```

Tap also has `with_on_secondary_tap*` and `with_on_tertiary_tap*` per button
(`TapButton`).

## Recognizers

State names below are the private phase enums in each file.

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
- After the window expires, `check_timeout()` releases the held entry (so a
  competing single tap can win), fires `on_double_tap_cancel`, and returns to
  `Ready`; a new contact then counts as a first tap.
- Movement beyond slop during `FirstDown` or `SecondDown` cancels.

Builders: `with_on_double_tap`, `with_on_double_tap_down`,
`with_on_double_tap_cancel`.

### Long press (`long_press.rs`, `LongPressPhase`)

`Ready → Possible` on down; `Possible → Started` when `long_press_timeout()`
elapses within slop; movement beyond slop before that cancels. Firing goes
through one path, `try_fire_timer`, which is reached from frame deadline
polling, `handle_move`, and the public `check_timer()`. It accepts the arena
entry before invoking `on_long_press` / `on_long_press_start`, so a competing
tap on the same region is already rejected when the callbacks run.

Builders: `with_on_long_press_down`, `with_on_long_press`,
`with_on_long_press_start`, `with_on_long_press_move_update`,
`with_on_long_press_up`, `with_on_long_press_end`, `with_on_long_press_cancel`.

### Drag (`drag.rs`, `DragPhase`)

`Ready → Possible` on down. The drag starts (`Started`) when the recognizer
wins its pointer's arena; moving past `pan_slop_for(kind)` along the axis
claims the win while competitors remain. `DragStartBehavior::Start` (default)
reports the position at acceptance, `Down` the down position. Up or an accepted
Cancel ends the drag; rejection before acceptance fires `on_cancel`.

```rust
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
`min_fling_velocity()`.

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

### Force press (`force_press.rs`, `ForcePressPhase`)

Down with pressure `0.0` means the device reports no pressure: `Ended`
immediately. Otherwise `Possible`, then `Started` at `start_pressure`
(`FORCE_PRESS_START_PRESSURE = 0.4`) and `Peaked` at `peak_pressure`
(`FORCE_PRESS_PEAK_PRESSURE = 0.85`); both are configurable with
`with_start_pressure` / `with_peak_pressure`. Dropping below the start pressure
from `Peaked`, pointer Up, or movement beyond `hit_slop(kind)` ends a started
press; movement beyond slop while `Possible` rejects it silently.
`ForcePressDetails { global_position, local_position, pressure, max_pressure }`.

### Multi-tap (`multi_tap.rs`, `MultiTapPhase`)

`new(arena, n)` recognizes `n` simultaneous contacts that stay within slop and
are all released. Each contact is tracked by its own pointer identity
(`multi_contact_events_keep_independent_pointer_identity`).

### Trackpad pan-zoom (`pan_zoom.rs`)

`convert_gesture` / `from_w3c_event` map an upstream `PointerGesture` to a
`PointerPanZoomEvent::Update`. The upstream event carries one tick, so `scale`
(`1.0 + pinch`) and `rotation` are per-tick deltas, not values accumulated
since a `Start`; `pan` and `pan_delta` are always zero.

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
Builders: `with_touch_slop`, `with_pan_slop`, `with_double_tap_timeout`,
`with_long_press_timeout`, `with_min_fling_velocity`, `with_max_fling_velocity`
and others.

## Arena integration

A recognizer joins the arena from `add_pointer`, which receives the owning
`Arc` so the registered identity is the recognizer itself
(`RecognizerBase::start_tracking(pointer, position, global_position, self)`).
`GestureArena::add` returns a `GestureArenaEntry` whose `resolve`, `hold`,
`release` and `sweep` act on that one membership. The arena calls
`accept_gesture` / `reject_gesture` after releasing its own locks.

At runtime `GestureBinding` hit-tests on Down, calls `add_pointer` along the
route, closes the arena, sweeps on Up, and does not sweep on Cancel.

## Velocity tracking (`processing/velocity.rs`)

```rust
let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
tracker.add_position(time, position);       // Offset<f64>
let v: Velocity = tracker.get_velocity();   // pixels_per_second: Offset<f64>
let fling = tracker.get_fling_velocity(false);
let estimate = tracker.get_velocity_estimate(); // Option<VelocityEstimate>
```

A quadratic least-squares fit over at most 20 samples within a 100 ms horizon;
fewer than 3 samples gives no fit, and a pointer still for 40 ms reports zero.
`IosFlingVelocityTracker`, `MacosFlingVelocityTracker` and
`ImpulseVelocityTracker` are alternative strategies with the same shape.

## Custom recognizers

Implement `CustomGestureRecognizer` (`on_arena_accept`, `on_arena_reject`) and
`GestureRecognizer`. In `add_pointer`, pass `self` (the `&Arc<Self>`) to the
arena. Building a new `Arc` from a clone registers a different allocation:
its entry handle goes stale once the arena resolves, and timers after
resolution cannot reach the recognizer (`GestureRecognizer::add_pointer` docs).

```rust
impl GestureRecognizer for TripleTap {
    fn add_pointer(self: &Arc<Self>, pointer: PointerId,
                   position: Offset<f64>, global: Offset<f64>) {
        self.base.start_tracking(pointer, position, global, self);
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        // measure with dispatch.local; report dispatch.global
    }
    fn dispose(&self) { self.base.mark_disposed(); }
    fn primary_pointer(&self) -> Option<PointerId> { self.base.primary_pointer() }
}
```

## Ownership and threading

Recognizers, the arena and their callbacks are owner-local (ADR-0027):
callbacks are `Rc<dyn Fn>` and may capture `Rc<Cell<_>>` state. Pointer
events, IDs and settings stay `Send + Sync` where they cross runtime
boundaries.

## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [HIT_TESTING.md](HIT_TESTING.md)
- [PERFORMANCE.md](PERFORMANCE.md)
