### Removed

- `flui_rendering::binding` and its `RendererBinding` trait, whose only implementation was `flui_runtime::renderer_binding::RenderingBinding`. The methods production calls (`root_pipeline_owner`, `send_frames_to_engine` and the hit test) are inherent methods of `RenderingBinding`.
- From the binding, as nothing in production called them: the render-view map (`with_render_view`, `insert_render_view`, `render_view_ids`, `remove_render_view_by_id`, `add_render_view_with_config`), `create_view_configuration_for`, `handle_metrics_changed`, `request_visual_update` and the scheduler it held, and the `debug_dump_render_tree`, `debug_dump_layer_tree` and `debug_dump_pipeline_owner_tree` functions with their `flui_rendering::prelude` entries.
- `RenderingBinding::{set_semantics_enabled, semantics_enabled, add_semantics_enabled_listener, remove_semantics_enabled_listener}`: nothing set the flag, so the listeners each presentation registered never ran. A presentation's semantics are enabled through its `SemanticsHost`, which the platform accessibility bridge sets.

### Changed

- `RendererBinding::hit_test_in_view(result, position, kind, view_id)` is `RenderingBinding::hit_test(result, position, kind)`: the view id was never read, since the walk always ran over the root pipeline owner. Its answer `ViewHitTest` moved from `flui_rendering::binding` to `flui_runtime::renderer_binding`.
- `RenderingBinding::new_with_pipeline` takes only the pipeline, and the binding's first-frame state is `Cell`s on the owner thread.
