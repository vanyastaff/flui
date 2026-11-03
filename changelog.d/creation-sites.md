### Added
- `CreationSite` in `flui-foundation`: where a value was created, recorded by `#[track_caller]` constructors. Kept in debug builds and, in release, with the `creation-site` feature; zero-sized otherwise. `View::creation_site` and `#[view(site)]` on a `CreationSite` field of a `StatelessView` or `StatefulView` derive report it, the element copies it at mount, and the element inspector shows it as `created_at`.
- `TreeContext`, `PathEntry` and `TreeError<E>` in `flui-foundation`: an error with the capped path of ancestors to the node it names and where that node's view was created. `PipelineOwner::tree_context` builds the context, `RenderError::node` names the node an error carries, and render nodes keep the creation site of the view that made them.

### Changed
- `PipelineOwner::run_frame`, `PipelineCell::run_frame`, `WidgetsBinding::run_frame_with_layout_builders` and `settle_layout_with_builders` return `TreeError<RenderError>`. `FrameFailureKind::Pipeline::error` is a `TreeError<RenderError>` that reads as the wrapped error through `Deref`, and its text names the node's ancestors.
- `flui-animation`'s `diagnostics` feature enables `flui-foundation/creation-site`; its controller creation site is the shared `CreationSite`.
