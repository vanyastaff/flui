### Changed

- `AlignmentDirectional::resolve`, `AlignmentGeometry::resolve`, `BorderDirectional::resolve` and
  `BorderRadiusDirectional::resolve` take a `TextDirection` instead of a `bool`.

### Removed

- `flui::painting::paint::{PaintingStyle, PathOperation, VertexMode, BlurMode, BlurQuality,
  PathPaintMode, StrokeOptions, ImageConfiguration, MaskFilter, BlurStyle, ClipBehavior}`: nothing
  drew with them. A paint's fill or stroke is `PaintStyle`, its stroke settings are `Paint`'s own
  fields, and a clip's quality is `Clip`.
- Predicates and helpers that nothing called: `BoxFit::{may_scale, may_scale_up, may_scale_down,
  may_clip, may_leave_space, maintains_aspect_ratio, fills_target}`, `BoxShape::{is_rectangle, requires_clipping}`,
  `FittedSizes::needs_scaling`, `BlendMode::{can_darken, can_lighten,
  is_compositional, requires_destination}`, `PointMode::{is_points, is_lines, is_polygon,
  min_points}`, `Paint::{has_dash, is_anti_aliased, is_fill, is_stroke}`, `PaintStyle::{is_fill,
  is_stroke}`, `Shader::has_stops`, `ImageShader::effective_filter_quality`,
  `ImageFilter::overflow_indicator`, `Image::byte_count`, `Color::blend_over_batch`,
  `BoxShadow::to_shadow`, `TextBaseline::is_ideographic`, `TextAlign::{is_direction_dependent,
  horizontal_factor}`, `TextAlignVertical::vertical_factor`, `LineMetrics::{has_hard_break,
  total_height}`, `TextSpan::{text_length, total_span_count}`, `FontWeight::is_bold` and
  `GlyphKey::with_hinting`.
- `InlineSpanTrait` and `InlineSpan::as_trait`: `InlineSpan`'s `style`, `to_plain_text` and
  `has_semantics` match its two variants.
- The public modules `flui_painting::canvas::{clipping, drawing, scoped, state, transform}`, which
  held only `Canvas` methods.

### Fixed

- Recording a canvas no longer opens a tracing span at the default `info` filter on every
  `Canvas::finish`, `Canvas::take_recording` and `Canvas::save_layer`, which ran on every repaint.
