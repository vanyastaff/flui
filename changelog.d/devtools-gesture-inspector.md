### Added

- `flui_devtools::gesture_inspector::GestureInspector` reports what a gesture arena decided, per contest: the members that joined with their composition paths, claims, the winner, rejections with their reasons, elapsed deadlines and the input sequence. A recording is inspected by replaying it with the inspector attached.
- `HeadlessHost::arena` in `flui-testing`, the primary window's gesture arena, for observers.

### Changed

- `GestureObservation::Rejected` carries a `RejectionReason` (withdrew, defeated, cancelled, hold failed, unresolved), and `GestureObservation::Joined::composition` is the member's whole composition path as `ObservedComposition`s (kind, `CompositionBranch` and node identity), innermost first, instead of the innermost kind. Both are re-exported through `flui-sdk` and the facade.
