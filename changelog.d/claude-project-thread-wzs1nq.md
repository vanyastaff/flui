### Added

- `flui::rendering::objects`: the built-in render objects (`RenderAlign`, `RenderFlex`,
  `RenderSliverList`, …) and the configuration types they take, as Flutter's rendering library
  exports them. Each built-in widget's `RenderView::RenderObject` is now a path an author can write.
- `flui::rendering` re-exports the render-contract types its own items name: the `Protocol`
  associated types (`BoxLayout`, `SliverLayout`, `ProtocolConstraints`, `ChildState`, …),
  `LayoutContext`, `HitTestContext`, `PaintAnchor`, `PaintFragment`, `FragmentRecorder`,
  `GrowthDirection`, `InputRegionSink`, `SemanticsClip`, `PoisonPhase`, `DirtySendError` and the
  parent-data types a sliver cross axis takes. Each was reachable only by an internal crate path.
- `flui::semantics`: the vocabulary `SemanticsConfiguration` takes, as Flutter's semantics
  library exports it: tags, sort keys, hint overrides, relations, collections, states, text
  content, custom actions and their handlers, announcements. Each was reachable only by an internal
  crate path. `flui::painting` adds `SemanticsRegionId` beside the region types that name it.

### Removed

- `RawId::zip_unchecked`, `Id::zip_unchecked` and `Id::new_unchecked`, the hidden `unsafe`
  constructors of `flui::foundation` ids; the safe constructors stay.
- `flui_sdk::pipeline::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction}`: they are
  `flui_sdk::rendering::objects::*`, at the facade's path.

### Changed

- devtools' `Verdict` and `EventCategory` are `#[non_exhaustive]`, so a `match` on one needs a `_`
  arm (ADR-0329).
