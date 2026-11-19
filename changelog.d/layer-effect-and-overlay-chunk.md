### Added

- **`flui-layer`**: `Effect`, the layer a retained scene's content opens: any `Layer` but a
  picture. `ContentBuilder::push_effect` takes `impl Into<Effect>`, so a picture effect no longer
  compiles; `Effect::try_from(Layer)` hands a picture back as its error.

### Changed

- **`flui-runtime`**: the performance overlay is a node of the presentation's scene holding one
  chunk of its readout under a clip to its bounds. A frame whose numbers, fonts and device pixel
  ratio are unchanged keeps that chunk and damages nothing; new numbers re-record that chunk alone.

### Removed

- **`flui-layer`**: `PerformanceOverlayLayer`, `PerformanceOverlayOption`, `Layer::PerformanceOverlay`
  and `Layer::as_performance_overlay`; the overlay is recorded content, not a layer.
