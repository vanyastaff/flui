### Removed

- `flui-view`'s `runtime-internals` feature. Its items are at `flui_view::__runtime`, which has no semver promise and is not reachable through `flui::view` or `flui_sdk::view`; the four `WidgetsBinding` methods it gated (`with_global_key_registry`, `draw_frame_with_phase_marker`, `lifecycle_source`, `notify_committed_lifecycle`) are methods of the sealed `flui_view::__runtime::BindingRuntime` trait.
