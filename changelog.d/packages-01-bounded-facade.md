### Changed

- `flui::app` is now a bounded module containing only cross-thread application control and host executor contracts. Ordinary launch, configuration and window-creation APIs remain at the crate root and in `flui::prelude`.
- Generated empty, basic and counter applications enable the facade's `native` feature only for Android.

### Removed

- The whole-crate `flui::app` alias, including its runner, bindings, embedder and second prelude, is no longer public facade surface.
- `flui::android_activity` and `flui::run_app_android` moved to `flui::native::android::{AndroidApp, run_app_android}`, available on Android with the `native` feature.
