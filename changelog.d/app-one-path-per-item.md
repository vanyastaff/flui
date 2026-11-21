### Changed

- `flui-app`: every public item is at the crate root once. The `flui_app::app` and `flui_app::app::runner` modules are no longer public, so `flui_app::app::register_font` and `flui_app::app::runner::register_font` are `flui_app::register_font`, and `flui_app::app::AppConfig` is `flui_app::AppConfig`; `flui_app::app::DevReload` is now `flui_app::DevReload`. The facade's paths (`flui::app`, `flui::AppConfig`) are unchanged.
- `flui_app::AppConfig`'s fields are no longer public: an application sets them through the builder methods, which carry the field names (`AppConfig::new().title("Notes").size(900, 700)`). The struct is no longer `#[non_exhaustive]`, since nothing can construct it by literal.

### Removed

- `flui_app::bindings`, `flui_app::embedder` and `flui_app::prelude`, with the re-exports they carried at the crate root: `GestureBinding` (now `flui_interaction::binding::GestureBinding`), `PipelineOwner` and `PipelineCell` (`flui_rendering::pipeline`), `UpdateScheduler` (`flui_scheduler::UpdateScheduler`), `WidgetsBinding`, `BuildOwner` and `ElementTree` (`flui_view::__runtime`, internal), `RenderingBinding` (`flui_runtime::renderer_binding::RenderingBinding`, internal) and the `tracing` macros the prelude re-exported (`tracing::{debug, error, info, trace, warn}`).
- `flui_app` no longer re-exports `flui_view`'s `BuildContext`, `BuildContextExt`, `ElementBase`, `StatefulView`, `StatelessView`, `View` and `element::StatelessElement`; use `flui_view::…` or the facade's `flui::view`.
