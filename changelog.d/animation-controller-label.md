### Added

- **Controllers carry a debug label and creation site** (`flui-animation`):
  `AnimationControllerBuilder::debug_label` and `AnimatedValue::with_debug_label` name a controller,
  and `build`/`build_on`/`AnimatedValue::new` record their caller (with debug assertions or the
  `diagnostics` feature; zero-sized otherwise). Both appear as `label` and `created_at` on the
  `animation_controller` span, on refusal and non-finite warnings, and in `Debug` output. With the
  `diagnostics` feature, those warnings are parented to the controller span. They are display
  fields: correlation stays on span identity.
