# ADR-0143: FLUI owns its pointer and keyboard event vocabulary

- **Status:** Accepted
- **Date:** 2026-10-06
- **Related:** [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §4 (this record is
  its input vocabulary, with the generator and the gate §4 asks for),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) (the contract crate that holds it),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (logical `f64` geometry),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (text input travels through the text
  store, not key events), [ADR-0038](ADR-0038-data-transfer-architecture.md) (drag and drop stays outside
  the pointer pipeline)

## Context

Before the migration, `flui_platform_api::PlatformInput` carried `ui_events::pointer::PointerEvent` and
`ui_events::keyboard::KeyboardEvent`, and the crate re-exported them with `ScrollDelta`, `Key`
and `Modifiers` (`crates/flui-platform-api/src/input.rs:28-38`). `flui-platform-api` is a
Stable crate; ADR-0089 forbids upstream types in its signatures and names this vocabulary as
the first thing to replace (§4). The replacement also has to carry what the upstream shape
cannot, which the interaction audit's market matrix lists against the code:

- **Units and sensors lie.** Positions travel in `dpi::PhysicalPosition` holding logical pixels
  (`input.rs:94-106` documents the trap); a mouse reports pressure 0.5 while pressed
  (`flui-platform/src/platforms/windows/events.rs:182`), indistinguishable from a real
  half-pressure; an untilted pen and a pen without tilt sensing read the same default.
- **Missing fields.** No eraser tool distinct from a button, no twist, no persistent device
  id with an accessor, no keyboard timestamp, no scroll phase or momentum, no inertia cancel,
  no pan-zoom start and end, no cancel reason, no device added/removed.
- **Erased information.** A scroll's unit is resolved to pixels with fixed 53 px lines and
  400 px pages before any scrollable sees it (`flui-interaction/src/events.rs:530-548`); a
  precise trackpad and a notched wheel are indistinguishable.

## Decision

### 1. Where and what

`flui-platform-api` defines the vocabulary in two public modules and one root type:

- `pointer`: `PointerEvent` (`Down`, `ButtonChange`, `Up`, `Move`, `Cancel`, `Enter`, `Leave`,
  `Scroll`, `ScrollInertiaCancel`, `PanZoom`, `DeviceAdded`, `DeviceRemoved`), `PointerInfo`
  (`PointerId`, `Option<DeviceId>`, `PointerKind`, `PointerRole`), `PointerKind` (`Mouse`,
  `Touch`, `Pen { tool: PenTool }`, `Trackpad`, `Unknown`), `PointerButton`/`PointerButtons`,
  `PointerSample`, `PenOrientation`, `PointerButtonEvent<D>` (`PointerPress`, `PointerRelease`, typestate over a sealed `ButtonDirection`), `ButtonChange`, `PointerMove`, `PointerSignal`,
  `PointerCancel` with `CancelReason`, `PointerDeviceChange`, `ScrollEvent` with `ScrollDelta`,
  `ScrollPrecision` and `ScrollPhase`, and `PanZoomEvent` with `PanZoomPhase` and
  `PanZoomTransform`.
- `keyboard`: `KeyEvent`, `Key`, `NamedKey`, `Code`, `KeyState`, `Location`, `Modifiers`.
- `EventTime`: nanoseconds on one monotonic timeline per process, shared by every backend.

`PlatformInput` and the root re-exports use these owned types. Every enum that can grow and every event
struct is `#[non_exhaustive]`; event structs are built through a constructor and by-value
`#[must_use]` `with_*` methods. A field whose value stands alone is public and its type carries
the invariant (§3), so no field can hold an unchecked number. Fields that must agree with each
other are private and read through accessors, so they cannot be set apart: a button event's
changed button and held set, a move's current reading and its coalesced and predicted
history, a key event's state and repeat, and a pan/zoom event's pointer, whose kind is always
`Trackpad`.

### 2. Semantics fixed by the types

- **Geometry is logical `f64`** (ADR-0098): positions are `Point<f64>`, contact sizes
  `Size<f64>`, pixel deltas and pans `Offset<f64>`, all in the receiving window's logical
  pixels. A backend whose platform reports device pixels divides before building the value.
- **A missing sensor is `None`.** Pressure, tangential pressure, orientation, twist and
  contact size are `Option`s; a mouse's pressure is `None`, not the W3C's 0.5. A consumer
  that wants the W3C default chooses it. Angles are radians: altitude in `[0, π/2]`, azimuth
  and twist in `[0, 2π)`, clockwise on screen.
  Altitude and azimuth are independently optional inside `PenOrientation`: `try_new`
  validates both reported angles, while `try_altitude` and `try_azimuth` preserve a single
  reported angle. An orientation always contains at least one reading. A backend validates
  readings independently and omits an invalid angle without discarding the other valid
  reading; it never fabricates zero for a missing angle.
- **A sequence ends explicitly.** A contact is `Down`, `Move`s and `ButtonChange`s, then `Up`
  or `Cancel`. A second button on a held pointer is a `ButtonChange`, not a new `Down`
  (W3C chorded buttons). The direction is in the type: `Down` takes a `PointerPress`, `Up` a `PointerRelease`, so a
  release cannot start a sequence. An event's `buttons` is the set after the change, and its
  constructor makes that true. `CancelReason` says why (`Platform`, `CaptureLost`,
  `FocusLost`, `DeviceRemoved`, `InvalidInput`); every reason ends the sequence the same way,
  without a tap or a fling. A backend cancels a removed device's contacts before
  `DeviceRemoved`, and a consumer that still holds one ends it there.
- **The pen eraser is a tool.** `Pen { tool: Eraser }` can hover and touch like the tip and
  can flip mid-hover or during a contact, so it is not a button; the W3C eraser bit has no `PointerButton`.
- **Primary is a role, not an id.** `PointerInfo::role` is the W3C `isPrimary`;
  `PointerId` values carry no meaning and may be reused after a sequence ends.
  A cached contact admits packets by pointer id and device identity, not by mutable
  kind/tool or role. Coalescing requires the full `PointerInfo` to match because
  individual samples do not carry their old metadata; a metadata boundary delivers
  both packets separately and preserves accepted delivery debt during reentry.
- **Coalesced and predicted readings** travel on `PointerMove`, oldest first, before and after
  `current`; the constructor orders and filters them.
- **A scroll keeps its unit.** `ScrollDelta` is lines, logical pixels or pages, positive
  scrolling down and right (W3C sign); the scrollable that receives it resolves lines and
  pages. `ScrollPrecision` says notched or precise; `ScrollPhase` carries the gesture
  (`Began`, `Changed`, `Ended`, `Cancelled`) and the platform's momentum (`MomentumBegan`,
  `MomentumChanged`, `MomentumEnded`) in one enum, so "fingers down and momentum" cannot be
  represented; a wheel tick outside a gesture has no phase. `ScrollInertiaCancel` is the
  finger that stops the platform's momentum.
- **A trackpad gesture is one cumulative stream.** `PanZoomPhase` is `Start`, `Update`, `End`
  or `Cancelled`; an `Update`'s `PanZoomTransform` (pan, scale, clockwise rotation) is
  cumulative since `Start`, not a step. The producer accumulates the platform's steps once,
  in `f64`, so a recognizer joining mid-gesture sees the whole gesture and rounding does not
  compound in every consumer. The scale is always positive.
- **Keys** follow UI Events: `Key` is what the key means (`Character` or `Named`), `Code` the
  physical key, `Location`, `KeyRepeat` (never `AutoRepeat` on a release), `ImeComposition`
  (an active one means the input method owns the key), the modifiers and an `EventTime`. Text input is not a key event (ADR-0090).
  The legacy `Hyper` and `Super` are `Meta`.

### 3. Numbers enter through validated types

Every number in the vocabulary is a validated value: `PointerPosition` (finite `Point<f64>`),
`Pressure` (`[0, 1]`), `TangentialPressure` (`[-1, 1]`), `PenOrientation` (altitude in
`[0, π/2]`, azimuth wrapped), `Twist` (wrapped), `ContactSize` (finite, not negative),
`ScrollDelta` (unit plus finite components) and `PanZoomTransform` (finite, positive scale).
Each has a `try_new` (and `TryFrom` where a source type exists) returning a `thiserror`
`InputValueError` that names the `Quantity`: NaN or an infinity is always `NonFinite`; a
finite value outside a bounded range is `OutOfRange`; clamping happens only through an
explicit `saturating` constructor (pressures, which devices overshoot by rounding); periodic
angles are wrapped into `[0, 2π)`. Event constructors take these types and are therefore
infallible. Later coordinate transforms and unit resolution validate their intermediate
arithmetic and final output: finite source values can still overflow. IDs are `NonZeroU64`
newtypes, flags are enums (`PointerRole`, `KeyRepeat`, `ImeComposition`),
and a button number outside 1–5 and 7–32 is an `InvalidButtonNumber`. A producer that cannot
build a release (its position is not finite) emits `Cancel { reason: InvalidInput }`, so the
sequence still ends and no tap lands at a guessed position. A nonfinite scroll distance
cannot erase an admitted terminal phase: a phased producer preserves the phase with zero
distance in the same unit. An unphased invalid wheel tick may be refused. A failed local
coordinate transform skips that local delivery without pretending global coordinates are
local; global contact retirement still settles its terminal obligation.

### 4. Generated key tables and their gate

`NamedKey` and `Code` are generated by `cargo xtask key-vocabulary --write` from the
direct `keyboard-types` generator dependency pinned by xtask, with the upstream documentation and an `ALL`
table, `as_str` (the W3C spelling) and `from_w3c`. `cargo xtask checks` runs
`key-vocabulary --self-test` and `key-vocabulary`, which fails when the checked-in tables are
not what the pinned `keyboard-types` generates. Upstream variants marked `#[deprecated]` are
not generated; any other unknown attribute fails the generator. A key the specification adds
before regeneration arrives as `Unidentified`, the documented loss ADR-0089 §4 accepts.

### 5. Native producers and audited keyboard adapters

Backends produce owned pointer events directly. They read native sensor presence, source
time, units/phases and contact identity at their boundary; no W3C pointer-default bridge
remains. Web wheel events carry no pointer id, so that producer deliberately uses the
primary mouse fallback. Missing native capabilities stay absent instead of acquiring
synthetic readings or phases.

AppKit's two-finger pan arrives through `scrollWheel:` and remains a `ScrollEvent`
with its native finger or momentum phase and `hasPreciseScrollingDeltas` precision.
It does not also emit `PanZoom`. The native `magnifyWithEvent:` and
`rotateWithEvent:` callbacks share one cumulative `PanZoom` sequence; the producer
accumulates scale and rotation and ends the sequence when its native components
finish. This distinction preserves the input AppKit actually reports without
inventing a second pan stream or a gesture phase for an ordinary wheel tick.

Win32 wheel precision describes observed packet granularity, not hardware identity:
a distance that is not a multiple of `WHEEL_DELTA` is `Precise`, while whole-step
and zero packets remain `Unknown`. Both retain their signed line unit and actual
hover source. An integral packet alone cannot identify a notched device.

Winit's complete keyboard-event mapping and Android's native keycode tables remain in
their maintained ecosystem adapters. `flui-platform`'s private `shared::keyboard_adapter`
converts those results to owned key events without leaking upstream types through Stable
signatures. Win32 and AppKit key tables return owned enums directly; Web resolves DOM W3C
spellings against the generated owned tables. The dependency retention is deliberate:
duplicating complete audited keyboard mappings would add a second maintenance owner.
`keyboard_adapter_contract` pins field preservation, canonical spellings and legacy Meta
aliases. The generator's direct dependency makes its gate independent of the runtime
adapters; their removal or target-feature selection cannot change the authoritative tables.

### 6. Migration sequence

1. The types, the generator and gate, and the bridge, with no behaviour change.
2. `PlatformInput` and its consumers (`flui-interaction`, `flui-runtime`, the facade's input
   re-exports) switch to the vocabulary in one change; backends convert through the bridge.
   `flui-interaction`'s own copies (`PointerDeviceKind`, `PointerPanZoomEvent`, the scroll
   resolution at fixed pixel sizes) fold into it.
3. Each backend produces the vocabulary directly, filling what the bridge cannot (OS
   timestamps, phases, momentum, real "no sensor", eraser, device changes), one backend per
   change; `ui-events`, `keyboard-types` and `dpi` then leave the dependency graph of the
   Stable crates. Audited keyboard adapters may retain upstream dependencies inside the
   backend layer where they remain genuine production consumers (§5); Winit also retains
   its native physical window geometry dependency.

## Alternatives considered

- **Keep `ui-events` and wait for it to grow the missing fields.** Rejected: it breaks
  ADR-0089, ties a Stable crate to a dependency with four breaking releases since 2025-05
  (ADR-0089's count), and its current shape (no scroll phase, `PhysicalPosition` for logical values, 0.5 pressure) is the gap.
- **Newtype wrappers around the `ui-events` structs.** Rejected: hides the type names but
  keeps the semantics (no "no sensor", no unit, no phase), and every accessor delegates to a
  release cadence FLUI does not control.
- **Mirror winit 0.31's event types.** Rejected as a template, kept as a checklist: its
  `PointerKind`, `ButtonSource`, `Force` and gesture events informed the variants, but winit
  is a windowing library whose events are per-window callbacks with device pixels, and its
  pinch, pan and rotation arrive as three separate delta streams (`PinchGesture`,
  `PanGesture`, `RotationGesture`, each a `delta` with its own phase, in the pinned 0.30).
- **Per-event deltas for pan/zoom.** Rejected: every consumer would accumulate, a recognizer
  that joins late would lose the start, and the platforms that report a cumulative transform
  (Windows Direct Manipulation) would have to differentiate. Flutter's
  `PointerPanZoomUpdateEvent` is cumulative too ("the total pan offset", "rotated in radians
  so far": `packages/flutter/lib/src/gestures/events.dart`, read 2026-10-06), and adds a
  `panDelta` that FLUI leaves to the consumer rather than carry two values that can disagree.
- **`f32` sensor values everywhere, or `f64` everywhere.** Normalized sensor scalars stay
  `f32` (the platforms report them so, and no arithmetic compounds them); geometry and angles
  are `f64` (ADR-0098).

## Consequences

- Two input vocabularies exist until step 2: the new types are unwired in production, which
  the step-2 change ends.
- A `keyboard-types` bump regenerates two files; a bump that renames a key fails `checks`
  until it does.
- Pre-1.0 consumers of the facade's input re-exports get FLUI's types at step 2: same
  concepts, different names in places (`KeyEvent`, `PointerKind`, `Pen { tool }`).

## Verification

The source audit for the pointer bridge uses
`rg -n 'ui_events::pointer|ui_events::ScrollDelta|input_vocabulary' crates/flui-platform/src crates/flui-interaction/src`.
Runtime dependency inspection uses `cargo tree -p flui-platform-api -e normal`;
backend adapters are inspected separately so a legitimate keyboard mapping or
Winit physical geometry dependency is not mistaken for a Stable signature leak.

- `flui-platform-api`'s `input_vocabulary_contract`: sanitization, "no sensor", clamping and
  wrapping, button sets, coalesced ordering, scroll units and phases, pan-zoom transform
  validity, key events, generated spellings.
- `flui-platform`'s `keyboard_adapter_contract`: every generated `NamedKey` and `Code`,
  legacy Meta aliases and preservation of keyboard event fields. Its private placement is
  documented in the crate's mapping decision: Winit's complete native `KeyEvent` contains
  inaccessible platform state.
- Native decoder contracts: Winit's `native_scroll_units_precision_and_phases` and
  `native_pan_zoom_phase_and_recovery_matrix`; Win32's
  `native_pointer_decoding_contracts`, including the rows
  `native_pen_masks_preserve_sensor_presence`,
  `native_touch_contact_and_pressure_are_measured` and
  `native_enter_before_down_preserves_first_contact_admission`; AppKit's
  `native_scroll_phases_preserve_momentum_and_unphased_wheels` and
  `native_pinch_and_rotation_share_one_cumulative_gesture`; UIKit's
  `native_touch_readings_keep_sensor_presence_and_pen_angles`; Android's
  `android_native_pointer_readings`. These pin the decoding seams used by real
  producers; compiling a target does not establish native execution or hardware
  delivery.
- Native window checks: Win32's
  `fractional_native_wheel_packets_preserve_observed_precision_and_source` queues
  actual wheel messages to an owned hidden window and checks fractional distances,
  precision, units and source preservation.
  `synthetic_touch_and_pen_reach_native_pointer_dispatch` exercises native injection
  only when the host admits it: foreground refusal, an occluded target or unavailable
  injection capability reports `CANNOT_VERIFY`, which is not proof of touch/pen
  delivery. `cargo xtask device windows-input` is an application mouse/keyboard
  smoke check, not a replacement for that touch/pen path. The browser input probe's
  `owned-wheel-check`, `owned-keyboard-check`, fractional, sensor, capture and reentry
  checks exercise its DOM producer separately. These references describe available
  checks; their inclusion here does not claim a successful run on the current host.
- `pointer_identity_contracts` pins source isolation, mutable contact metadata and
  delivery debt; widget `viewer_unstarted_pinch_updates_remain_independent_steps` retains
  the deliberate isolated-Update consumer behavior separately from native cumulative
  gesture producers.
- `cargo xtask key-vocabulary --self-test` and `cargo xtask key-vocabulary` in
  `cargo xtask checks`.
