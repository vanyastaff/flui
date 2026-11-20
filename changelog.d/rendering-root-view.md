### Changed

- **`flui-rendering`**: `view::RenderView` is the root render object itself (`RenderView::new()`); it takes the size from the pipeline owner's root constraints, as the runtime sets them each frame. `FragmentRecorder::new` and the recorder's and `PaintContext`'s ratio take and return a `DevicePixelRatio`, and `dpr()` is now `device_pixel_ratio()`.
- **`flui-view`**: `RootRenderView::new` takes only the child, and `WidgetsBinding::attach_root_widget_with_size` and `DEFAULT_ROOT_VIEW_SIZE` are gone: `attach_root_widget` is the one attach. The root element's update no longer fails silently on a downcast that never matched.
- **`flui-sdk`**: `hooks::PluginPipeline::mount(root, text)` takes no width and height, and `app_plugin!` mounts that way.
- **`flui-runtime`**: `UiRuntime::attach_root_widget_with_size(view, width, height)` and `attach_root_widget_with_size_to(id, view, width, height)` are `attach_root_widget(view)` and `attach_root_widget_to(id, view)`, since the root takes its size from the runtime's root constraints.

### Removed

- **`flui-rendering`**: `view::RenderViewAdapter` (use `view::RenderView`), `view::ViewConfiguration` (the root's constraints come from `PipelineOwner::set_root_constraints`, the ratio from `PipelineOwner::device_pixel_ratio`), and `RenderView`'s `with_configuration`, `set_configuration`, `configuration`, `has_configuration`, `constraints`, `layer`, `prepare_initial_frame_without_owner`, `perform_layout`, `physical_paint_bounds`, `apply_paint_transform` and `size`.
