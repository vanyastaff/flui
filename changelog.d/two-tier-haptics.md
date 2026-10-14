### Added

- **Two-tier haptics** (`flui-platform-api`, `flui-interaction`, `flui-view`, `flui-runtime`,
  `flui-platform`; ADR-0210): `HapticFeedback` gains `Alignment`, `LevelChange`, `DragLift`,
  `Drop`, `Confirm` and `Reject`; `HapticPattern`, built by a validating builder over
  `Intensity` and `Sharpness`, is the second tier. Both take a `HapticRequest` (source device,
  `Immediate` or `FramePresented` timing, input sequence) and report a `HapticOutcome`. A
  frame-aligned request returns `Queued(HapticTicket)`, plays with the next presented frame by its
  deadline (the platform's factor times the display's refresh period) or resolves `Suppressed`,
  and its outcome reaches the presentation's observer. Widgets acquire a `HapticsHandle` through
  `LifecycleContext::haptics`; `HapticsMode` switches haptics off for a subtree, and the user's
  haptics setting suppresses both tiers. macOS, iOS (tier 1), Android and the Web play natively
  (never run on a device yet); every backend's degradation table and the haptics conformance
  rows run headless. The second tier, requests and outcomes are re-exported by the facade and
  `flui-sdk` only under `unstable-input`.

### Changed

- **`FakeHaptics`** (`flui-platform`) now lives in `flui-platform-api::haptics` (still
  re-exported at its old path) and records each request with the actuator and outcome;
  `calls()` returns `HapticCall`s and `feedback()` the kinds that played.
