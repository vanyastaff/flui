### Added

- `flui::rendering::objects`: the built-in render objects (`RenderAlign`, `RenderFlex`,
  `RenderSliverList`, …) and the configuration types they take, as Flutter's rendering library
  exports them. Each built-in widget's `RenderView::RenderObject` is now a path an author can write.

### Removed

- `RawId::zip_unchecked`, `Id::zip_unchecked` and `Id::new_unchecked`, the hidden `unsafe`
  constructors of `flui::foundation` ids; the safe constructors stay.
- `flui_sdk::pipeline::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction}`: they are
  `flui_sdk::rendering::objects::*`, at the facade's path.
