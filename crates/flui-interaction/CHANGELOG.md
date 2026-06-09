# Changelog

All notable changes to `flui-interaction` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

- `EagerGestureRecognizer` — wins the arena on `add_pointer` (Flutter `eager.dart:42-68`). Use for `AndroidView` / `UiKitView` (HybridComposition) hit regions that must unconditionally absorb input.
- `pub mod observability` with `GestureEvent` typed enum + `SPAN_RECOGNIZER` / `SPAN_ARENA` span-name constants + `pointer_event_kind` helper. `#[tracing::instrument]` on `RecognizerBase` and `GestureArena` hot paths; observability is now Definition-of-Done for any recogniser or arena change.
- `MultiDragGestureRecognizer` — per-pointer drag with team / multiple handles (`multi_drag.rs`).
- `TapAndDragGestureRecognizer` — composite tap-then-drag (`tap_and_drag.rs`).
- `PointerPanZoom` variants — trackpad-fidelity precision + rotation.
- `SamplingClock` for the resampler; `PointerEventResampler` wired into `GestureBinding`.
- Criterion benchmarks under `crates/flui-interaction/benches/` — `velocity_tracker_bench`, `gesture_arena_bench`, `tap_detector_bench`, `pointer_resampler_bench`. Run with `cargo bench -p flui-interaction`. Hot-path regression guards; no `eprintln!` / `dbg!` in committed code.

### Changed

- `TapGestureRecognizer` now routes Primary / Secondary / Tertiary (`PointerButton::Primary` / `Secondary` / `Auxiliary`) to per-button callback slots. `on_secondary_tap_down/up/cancel` and `on_tertiary_tap_down/up/cancel` added to `TapCallbacks`. Button mismatch on Up cancels the in-flight primary tap (Flutter `tap.dart::_checkUp`).
- `VelocityTracker` gains `with_kind(PointerDeviceKind)` constructor + `get_fling_velocity` parity with Flutter. Legacy `velocity()` / `is_reliable()` shims kept for source compat.
- `lsq_solver` factored into a reusable helper (regression math shared with `PointerEventResampler` and `VelocityTracker`).
- drag recognisers split into `HorizontalDragGestureRecognizer` / `VerticalDragGestureRecognizer` / `PanGestureRecognizer` with `dragStartBehavior` and `multitouchDragStrategy` per-axis slop. `drag_variants` module added. Legacy `DragGestureRecognizer` is now an alias for the three-axis supertype.

### Fixed

- `LongPressGestureRecognizer::did_exceed_deadline` now calls `try_fire_timer` before resolving Accepted so `on_long_press_start` fires (was resolving without firing the start callback).
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
