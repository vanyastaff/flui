### Added

- The development agent answers `audit.a11y`: `unnamed` and `target_size` over the committed
  semantics tree, `text_contrast` over the last applied frame, and every rule it cannot run
  listed in `not_checked`, so such a report is never `pass`.
- The development agent answers `perf.start` and `perf.stop` with a `PerfReport` of the frames a
  window presented between them: percentiles, the slowest frames with their phases, and dropped
  and janky frames against the display's refresh period when it is known.
- `flui_semantics::MinimumTarget` and `SemanticsOwner::audit_unnamed` / `audit_target_size`: the
  target-size rule judges the hit region inside every clip, never the drawn bounds.
- `flui_rendering::audit` and `PipelineOwner::audit_text_contrast`: WCAG text contrast read from
  a frame's scene, with a text whose background the draw commands do not determine reported as
  not checked.
- `ShapedRun::font_size`, `ShapedRun::color` and `ShapedParagraph::span_weights` in
  `flui-painting`.
