### Changed

- `flui_sdk::hooks::PluginPipeline::draw_frame` takes a `Size` and refuses one that is not finite
  or is negative with `PluginFrameError::InvalidSize`, before anything is built. A NaN or negative
  size from the host used to reach the root's constraints and panic in layout.
- `PluginFrameError::Pipeline` and `PluginFrameError::PatchRefused` carry the pipeline's
  `TreeError<RenderError>` and the scene's `SceneError` (now `flui_sdk::hooks::SceneError`), each
  returned by `source()`, instead of their text.
