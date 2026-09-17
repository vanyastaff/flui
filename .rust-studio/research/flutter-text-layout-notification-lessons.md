QUESTION: What does Flutter's open RenderParagraph text-layout notification issue teach FLUI, and does FLUI already have the needed abstraction?

ANSWER: Flutter #140756 is a useful external warning: text layout has observable geometry beyond a paragraph's outer box size. FLUI has strong internal text geometry APIs through `TextPainter`, but no public/render-pipeline contract for notifying widget-layer/editor/overlay consumers when caret/selection/glyph geometry changes without a full box relayout. Filed #1091.

Evidence read on 2026-09-13:

- Flutter #140756 is open, P2, framework, typography. The thread evolved from "textAlign should mark layout dirty" into the stronger architectural requirement: `RenderParagraph` should expose a mechanism for listeners to learn when text layout changes.
- Important external lesson: blindly escalating to full relayout can be too expensive and can fail across relayout boundaries; the missing primitive is a text-geometry dependency/notification, not simply "always markNeedsLayout".
- FLUI code:
  - `crates/flui-painting/src/text_painter/mod.rs` defines `Invalidation::{None, Paint, Layout}` only.
  - `TextPainter::set_text_align` returns `Invalidation::Paint`, preserves the shaped layout cache, and recomputes `cache.paint_offset`.
  - `crates/flui-painting/src/text_painter/paint.rs` geometry APIs (`get_offset_for_caret`, `get_position_for_offset`, `get_line_metrics`, `get_boxes_for_selection`, `get_word_boundary`) read cached layout plus `paint_offset`.
  - `crates/flui-objects/src/text/paragraph.rs` maps `Invalidation::Paint` to `RenderUpdateImpact::PAINT`; `RenderParagraph` has no geometry revision/listener/handle.
  - `crates/flui-objects/src/text/editable.rs` owns caret/composing/selection geometry for `RenderEditable`, but that is not a general dependency contract for arbitrary widget-layer overlays.
- Duplicate search:
  - Existing #540 is broad EditableText selection/multiline work.
  - #1064, #1080, #1085 are separate text-layout correctness issues.
  - No existing FLUI issue specifically covered a text-layout geometry notification/revision contract.

Issue created:

- https://github.com/vanyastaff/flui/issues/1091

Architectural direction captured:

- Treat text geometry as a first-class retained product between box layout and paint.
- Consider a `TextLayoutRevision` / `TextGeometryRevision`, or a separate invalidation tier such as `TextGeometry`, so editor/overlay/a11y systems can cheaply respond to changed glyph/caret/selection geometry.
- Keep full relayout reserved for changes that affect box size, line breaking, baseline, or parent constraints.
- Ensure handles/revisions are owner-scoped and stale-safe across unmount/reparent.

Why this matters for FLUI:

Rich text editing, IME cursor areas, selection handles, search highlights, annotations, spelling underlines, magnifiers, toolbars, and accessibility ranges all need stable text geometry. Without a public contract, higher layers will either downcast into render objects, duplicate layout knowledge, or over-invalidate.
