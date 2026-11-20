### Removed

- `flui_rendering::binding` and its `RendererBinding` trait, whose only implementation was `flui_runtime::renderer_binding::RenderingBinding`: the methods callers used (`hit_test_in_view`, `root_pipeline_owner`, `with_render_view`, `insert_render_view`, `send_frames_to_engine`) are inherent methods of `RenderingBinding`. `render_view_ids`, `remove_render_view_by_id`, `add_render_view_with_config`, `create_view_configuration_for` and `handle_metrics_changed` had no caller and are gone.
- `flui_rendering::binding::{debug_dump_render_tree, debug_dump_layer_tree, debug_dump_pipeline_owner_tree}`, which nothing called, and the `flui_rendering::prelude` entries for the trait, `ViewHitTest` and the dump functions.
- `RenderingBinding::{set_semantics_enabled, semantics_enabled, add_semantics_enabled_listener, remove_semantics_enabled_listener}`: nothing set the flag, so the listeners each presentation registered never ran. A presentation's semantics are enabled through its `SemanticsHost`, which the platform accessibility bridge sets.
- `RenderingBinding::request_visual_update`, which nothing called, and the scheduler it held for it.

### Changed

- `ViewHitTest` moved from `flui_rendering::binding` to `flui_runtime::renderer_binding::ViewHitTest`.
- `RenderingBinding::new_with_pipeline` takes only the pipeline; the binding's first-frame state is `Cell`s on the owner thread.
