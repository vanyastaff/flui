# Changelog

All notable changes to `flui-interaction` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

- `ImpulseVelocityTracker` — Android's default fling-velocity strategy since 8.1 (AOSP `VelocityTracker.cpp` impulse model: kinetic-energy bookkeeping, from-rest boundary condition). Flutter ships least-squares only; impulse discounts stale samples on sharp deceleration, tracking the finger's final intent.
- `OneEuroFilter` / `OneEuroFilter2D` — speed-adaptive low-pass for stylus/pointer smoothing (Casiez, Roussel & Vogel, CHI 2012) with the paper's recommended defaults (`min_cutoff=1.0`, `beta=0.007`, `d_cutoff=1.0`).
- `GestureSettings::for_platform(TargetPlatform)` (runtime platform dispatch, Flutter `defaultTargetPlatform` model) + cfg-seeded `GestureSettings::native()`; `android_defaults()` (AOSP `ViewConfiguration`: 8 dp slop, 16 dp paging, 300 ms double-tap, 400 ms long-press, 50–8000 dp/s fling) and `ios_defaults()` (10 pt `allowableMovement`; extrapolated fields documented).
- `DEFAULT_RESAMPLE_LOOKBACK` (38 ms) — Flutter's `samplingOffset` derivation, documented for callers driving `PointerEventResampler::sample`.
- `EagerGestureRecognizer` — wins the arena on `add_pointer` (Flutter `eager.dart:42-68`). Use for `AndroidView` / `UiKitView` (HybridComposition) hit regions that must unconditionally absorb input.
- `pub mod observability` with `GestureEvent` typed enum + `SPAN_RECOGNIZER` / `SPAN_ARENA` span-name constants + `pointer_event_kind` helper. `#[tracing::instrument]` on `RecognizerBase` and `GestureArena` hot paths; observability is now Definition-of-Done for any recogniser or arena change.
- `MultiDragGestureRecognizer` — per-pointer drag with team / multiple handles (`multi_drag.rs`).
- `TapAndDragGestureRecognizer` — composite tap-then-drag (`tap_and_drag.rs`).
- Exact-generation `GestureArenaEntry` handles and granular member withdrawal, so a recognizer can bow out without force-resolving peers or mutating a reused pointer slot.
- Re-exported `DragStartDetails`, `DragUpdateDetails`, `DragEndDetails`, `DragDownDetails`, plus `DragStartCallback`, `DragUpdateCallback`, `DragEndCallback`, `DragDownCallback`, `DragCancelCallback` from the crate root (previously only `DragGestureRecognizer` itself was exported).
- `PointerPanZoom` variants — trackpad-fidelity precision + rotation.
- `SamplingClock` for the resampler; `PointerEventResampler` wired into `GestureBinding`.
- Criterion benchmarks under `crates/flui-interaction/benches/` — `velocity_tracker_bench`, `gesture_arena_bench`, `tap_detector_bench`, `pointer_resampler_bench`. Run with `cargo bench -p flui-interaction`. Hot-path regression guards; no `eprintln!` / `dbg!` in committed code.

### Changed

- `TapGestureRecognizer` now routes Primary / Secondary / Tertiary (`PointerButton::Primary` / `Secondary` / `Auxiliary`) to per-button callback slots. `on_secondary_tap_down/up/cancel` and `on_tertiary_tap_down/up/cancel` added to `TapCallbacks`. Button mismatch on Up cancels the in-flight primary tap (Flutter `tap.dart::_checkUp`).
- `VelocityTracker` gains `with_kind(PointerDeviceKind)` constructor + `get_fling_velocity` parity with Flutter. Legacy `velocity()` / `is_reliable()` shims kept for source compat.
- `lsq_solver` factored into a reusable helper (regression math shared with `PointerEventResampler` and `VelocityTracker`).
- drag recognisers split into `HorizontalDragGestureRecognizer` / `VerticalDragGestureRecognizer` / `PanGestureRecognizer` with `dragStartBehavior` and per-axis slop. `drag_variants` module added. Legacy `DragGestureRecognizer` is now an alias for the three-axis supertype.
- `InputPredictor` maximum prediction horizon reduced 50 ms → 25 ms, aligned with Chromium's empirically validated `kMaxPredictionTime` (`ui/base/prediction/input_predictor.h`); longer horizons overshoot on direction changes.
- Consolidated the two parallel `DragAxis` enums (one in `traits.rs`, one in `drag.rs`) into `traits::DragAxis`; `drag_variants.rs` now imports from `traits`.

### Removed

- **Breaking:** `MultitouchDragStrategy` (enum, `with_multitouch_drag_strategy`, `multitouch_drag_strategy`). The drag recognizer is a single-sequence state machine with no per-pointer map, so `AverageBoundaryPointers`/`SumAllPointers` were silent no-ops behind a public builder — a contract violation. The surface returns together with a real multi-pointer drag implementation.

### Fixed

- **Deadlock:** `LongPressGestureRecognizer::handle_up` held the gesture-state lock across `stop_tracking()`; the arena sweep can synchronously reject the same recognizer, whose cancel path re-locks — guaranteed hang on lift-before-deadline with a competitor in the arena.
- **Deadlock:** `GestureArenaTeam` dispatched member callbacks while holding the combiner lock (`resolve`/`accept_gesture`/`reject_gesture`); a rejected recognizer's cancel path re-enters the same combiner via the team's arena wrapper. Notifications are now computed under the lock and dispatched after the guard drops.
- **Leak:** `GestureArenaTeam` held a strong `Arc` to itself (`self_ref`), a write-only reference cycle keeping every team alive for the process lifetime.
- `ScaleGestureRecognizer` routed every Move/Up to the primary pointer, so the second finger never updated its own slot and two-finger pinch produced no scale updates. Events are now routed by the event's own pointer id (Flutter `scale.dart` parity).
- `DoubleTapGestureRecognizer` inter-tap timeout now fires `on_double_tap_cancel` and releases the arena (Flutter `_reset → _checkCancel` parity); reset is atomic under one lock.
- `PointerEventResampler` computed interpolated positions but never emitted the synthesized Move while still advancing `last_position`; it now emits per Flutter `resampler.dart`.

- `LongPressGestureRecognizer::did_exceed_deadline` now calls `try_fire_timer` before resolving Accepted so `on_long_press_start` fires (was resolving without firing the start callback).
- **Arena bug:** `RecognizerBase::reject()` used to call `arena.resolve(pointer, None)`, which rejected every competing member. A tap that moved past its slop therefore silently killed the drag it was racing. `reject()` now withdraws only its exact-generation entry and clears local tracking, leaving remaining competitors untouched.
- pre-existing bug sweep — `events.rs::make_down_event_for_id` is now `#[cfg(any(test, feature = "testing"))]`-gated (was the only `*_for_id` family member shipping test-only code into the release binary); rustdoc hard-gate (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`) is green; `EagerGestureRecognizer` added to the crate-root `AssertSendSync` static-assertion block; `force_press.rs::with_start_pressure` / `with_peak_pressure` now `debug_assert!(Arc::strong_count == 1)` to document the builder-call-immediately-after-`Arc::new` contract; `processing::prediction::predict` and `processing::resampler` refactored from post-guard `.unwrap()` to `let-else`; `tap_and_drag.rs` replaces a production `expect` with a `let-else` + `tracing::warn!` recovery.

### Performance

- Gesture hot paths now carry typed `event = %GestureEvent::*` span fields. Filter via `RUST_LOG=flui_interaction::arena=debug,flui_interaction::recognizers=trace`.

## [0.1.0] - 2026-05-15

### Added

- Initial release of `flui-interaction` (Flutter gesture port).
- 9 recogniser types: Tap, LongPress, DoubleTap, Drag, Scale, ForcePress, MultiTap, MultiDrag, TapAndDrag.
- `GestureArena` + `GestureArenaTeam` + `PointerSignalResolver` dispatch.
- `VelocityTracker` (LSQ + exponential strategies).
- `PointerEventResampler` (frame-rate adaptation).
- `GestureBinding` (top-level event dispatch glue).
- `MouseTracker`, `FocusManager`, `EventRouter`, `PointerRouter` (input focus + routing).
- `GestureSettings` (device-specific tolerances).
- `prelude` module for convenient wildcard imports.
