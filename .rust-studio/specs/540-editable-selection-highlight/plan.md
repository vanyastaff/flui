# 540 slice 1 — selection highlight in `RenderEditable`

Issue #540 ("Finish EditableText selection, multiline, and obscured input").
Obscured input landed in PR #952. This slice is the next self-contained
piece: **paint a selection highlight**, which `crates/flui-objects/src/text/editable.rs`
currently names as deferred in its own module doc ("Deferred: selection
painting…", line 16) and whose absence blocks tap/shift-click/drag selection
above it.

## Reference contract (`.flutter/packages/flutter/lib/src/rendering/editable.dart`)

`_TextHighlightPainter.paint` (line 2905) is the whole contract:

- Paints nothing when the range is null, the colour is null, or the range
  **is collapsed** — the caret owns that case.
- Boxes come from `getBoxesForSelection` over the range.
- The boxes are **deduplicated into a `Set`** before drawing. With a
  translucent highlight colour a duplicate box double-blends, so this is
  observable, not incidental.
- Each box is shifted by the paint offset and **intersected with
  `Rect.fromLTWH(0, 0, textPainter.width, textPainter.height)`** — clipped to
  the text's own laid-out box, not to the render object's size.

Paint order (line 513): `_builtInPainters = [_selectionPainter, …]` are the
*background* painters, composed **before** `_textPainter.paint` at line 2627.
The highlight is behind the glyphs. FLUI's `paint` currently draws text first
(editable.rs:569), so the highlight goes ahead of that call, not after.

Caret ordering (`paintCursorAboveText`, line 508/521) is a separate axis —
out of scope for this slice; FLUI keeps painting the caret last, which is the
`paintCursorAboveText == true` arm.

## Change

`crates/flui-objects/src/text/editable.rs`:

- `selection: Option<Range<usize>>`, clamped through the same UTF-8-boundary
  path as `caret_byte_offset` and `composing_range`.
- `selection_color: Color`.
- `set_selection` / `with_selection` / `with_selection_color`, in the dedupe
  shape `set_composing_range` already uses (`RenderUpdateImpact::PAINT` on a
  real change, `NONE` otherwise).
- `paint`: highlight first, then text, then the composing underline, then the
  caret.

## Acceptance (observable)

1. Collapsed range → zero highlight draws. Control: a non-collapsed range on
   the same fixture draws at least one.
2. Highlight is drawn **before** the text — a display-list order oracle, not a
   pixel one.
3. Two identical boxes blend once, not twice (translucent colour).
4. A box extending past the laid-out text is clipped to the text box.
5. A byte offset landing mid-codepoint clamps to a boundary, matching
   `caret_offset_is_clamped_to_utf8_boundary`.
6. Setter dedupe/impact, matching
   `set_composing_range_dedupes_unchanged_and_invalidates_paint_on_change`.

Each test must fail with the corresponding production line reverted; that is
the bar, not "the suite is green".

## Out of scope, named

Gesture policy (tap / shift-click / drag), multiline layout and vertical
scrolling, `paintCursorAboveText`, and selection handles. Those are later
slices of #540 and depend on this one, not the reverse.
