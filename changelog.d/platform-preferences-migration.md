### Added
- `OwnerHost::update_preferences` makes a host's `SystemPreferences` the accepted revision and queues it for every runtime the host owns before any callback or wake runs; an unchanged value queues nothing. `OwnerHost::preferences` returns the latest accepted `owner::SystemPreferencesSnapshot`, and `RuntimeHostServices::with_preferences` seeds a runtime built later from it, so an older queued revision cannot replace it.
- `flui_sdk::platform` re-exports the rest of the system-preferences vocabulary: `DurationScale`, `GesturePreferences`, `InvalidPreference`, `MotionPreference`, `NativeMouseGeometry`, `NativeTouchGeometry`, `TextWeightPreference`, `WheelPreferences` and `WheelStep`.

### Removed
- `WidgetsBindingObserver::{did_change_locales, did_change_text_scale_factor, did_change_platform_brightness, did_change_accessibility_features}` and the `WidgetsBinding::handle_*_changed` broadcasts that fed them. Nothing in the host called them; system preferences reach widgets through `MediaQuery` from the host's `SystemPreferences` snapshot (ADR-0172).
- `RendererBinding::{handle_text_scale_factor_changed, handle_platform_brightness_changed}`, which did nothing.
