### Removed

- `flui_engine::Renderer`, the per-window renderer that only the `flui-hot-reload` example still
  used. Every window draws through a slot of one `GpuContext` (`RasterExecutor`,
  `GpuContext::bootstrap`, `SurfaceFactory::create`); a scene a test or tool draws without a window
  goes through `HeadlessRenderer`. `GpuCapabilities` is still exported from `flui_engine`.
- `EngineError::NotInitialized`, which only `Renderer` returned.
- The `scene_render` example of `flui-hot-reload` lost its built-in rectangles scene and its
  `--effects` window mode. It is now a FLUI application that installs `ScenePluginHook` through
  `AppConfig::dev_reload` (`cargo run -p flui-hot-reload --features host-hook --example
  scene_render`); `--capture-effects <png>` still writes the engine's effects gallery headless.
