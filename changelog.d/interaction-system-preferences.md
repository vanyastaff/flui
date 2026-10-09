### Changed

- Gesture recognizer builders accept fixed settings or a read-only live provider. Active contacts and retained gesture candidates keep the settings admitted for their sequence.
  Calls that previously relied on `.settings(Default::default())` should name `GestureSettings::default()`; the provider conversion accepts multiple input types. Pass an existing settings value directly instead of relying on an inferred `.into()` target.
- Native scale Begin captures its profile before the first movement. `ScaleGestureRecognizer::handle_pan_zoom` distinguishes refused admission, dormant admission and recognized delivery instead of returning an ambiguous boolean; a refused Begin cannot resume after an intervening touch contact ends.
- Drag and scale callbacks retain finite measured velocity independently of the admitted fling limit; `DragEndDetails::fling_velocity` and `ScaleEndDetails::focal_fling_velocity` supply the sequence's resolved pixel-speed policy. Scale-change velocity keeps its scale-per-second units. Public standalone velocity trackers retain their existing default ceiling.
  An unrepresentable measured component becomes zero independently, preserving finite motion on the other axis for callbacks and admitted fling.
- `InteractiveViewer` applies the touch or native sequence's captured fling range to its focal release inertia while preserving raw measurements in application callbacks.
- `GestureArenaScope` composes one inherited arena and input-policy node. Unconfigured nested scopes inherit gesture and wheel providers; explicit gesture settings override gesture policy independently. The public scope configuration no longer implements `InheritedView`; consumers acquire the arena through `GestureArenaScope::of`.
- `Scrollable` applies system wheel counts to raw detents, preserves normalized line, page and pixel input, and supports validated authored line and character distances. Invalid arithmetic refuses the packet without losing the previous scroll position.
- Raw Windows wheel packets use `ScrollUnit::Detents` instead of labeling each detent as a normalized line. Scroll consumers apply the system count once when resolving that unit.
- `GesturePreferences` retains `PartialEq` but no longer implements `Eq`, because native geometry carries validated floating-point measurements.

### Added

- Presentation-specific native gesture geometry queries distinguish unsupported observations from failed reads and retain their coordinate context.
