### Changed

- `PipelineOwner::new` and `new_with_capacity` take the `TextContextHandle` the pipeline measures through; every layout, intrinsic and dry query lends it (ADR-0092 §10 step 3b). A realm's presentations are built with the realm's context; a pipeline with no realm behind it passes `TextContextHandle::standalone()`.
- `RenderingFlutterBinding::new` and `PluginPipeline::mount` take a `TextContextHandle`, and `PluginPipeline::draw_frame` takes the frame's surface width and height. `app_plugin!` mounts the plugin pipeline with a context over the plugin image's own collection, not the host realm's.
- The raw text parameters of `RenderObject::intrinsic_raw`, `dry_layout_raw` and `dry_baseline_raw`, `Protocol::with_leaf_erased_ctx`, `RenderEntry::layout_leaf_only`, `RenderNode::layout_leaf_erased` and `ErasedBoxLayoutCtx::new` are a `TextSource<'_>`, no longer optional; `BoxLayoutCtx::new`, `with_children` and `with_layout_callback` take one too, and `BoxLayoutCtxErased::text_source` is a required method.

### Added

- `TextContextHandle::standalone`, a context over a font collection of its own, for a pipeline that is a realm by itself.
- `PipelineOwner::take_idle`, which moves an owner out of its slot and leaves an empty one that measures through the same context.

### Removed

- `PipelineOwner::set_text_context`, `PipelineOwner::with_callbacks`, `Default` for `PipelineOwner` and `RenderingFlutterBinding`, and `RendererBinding::create_root_pipeline_owner`. No pipeline measures on a text context it built for itself.

### Fixed

- A hot-reload plugin pipeline lays its tree out: `PluginPipeline::draw_frame` sets the root constraints to the surface size of each `flui_app_build` call, where no root constraints were set and an unmeasured tree was painted.
