### Removed

- **`Diagnosticable` and its machinery** (`flui-foundation`, `flui-macros`): `Diagnosticable`,
  `DiagnosticsBuilder`, `DiagnosticsNode`, `DiagnosticsProperty`, `DiagnosticLevel`,
  `DiagnosticsTreeStyle`, `#[derive(Diagnosticable)]` and the unused `DebugPaintConfig` are
  deleted. Render objects, layers, `DisplayList` and `GpuFrameProfile` describe themselves through
  `Inspect`, with typed fields (`Clip`, `BoxFit`, `AxisDirection`, `HitTestBehavior` and other enums
  derive it; `Alignment` and `RelativeRect` map to values). `PipelineOwner::debug_node_diagnostics`
  and `debug_diagnostics_tree` are replaced by `PipelineOwner::inspect()` with `snapshot`, and the
  render-tree kit's `Probe` reads typed fields (`render_snapshot`, `node_snapshot`).
