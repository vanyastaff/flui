# 540 — the slicing, corrected after reading the controller

Slice 1 (PR #960, merged) gave `RenderEditable` a selection to paint. The
obvious next slice looked like "wire gestures to it". It is not, and the
reason is worth recording because it is not visible from the issue text.

## The finding

**`TextEditingController` has no selection.** Its whole public surface is
caret-only: `caret_byte_offset`, `insert_str`, `backspace`,
`delete_forward`, `move_caret_{left,right,home,end}`, plus the composing-range
IME methods. There is no anchor/extent, no `set_selection`, and no
`TextEditingValue` type — #540's "keep `TextEditingValue` as the single
immutable edit transaction containing text, selection, and composing range"
describes a design that does not exist yet.

So a gesture layer has nowhere to write a selection to. Wiring drag-select
before the controller can hold one would mean either inventing a parallel
selection store in the widget (two sources of truth for the same fact) or
pushing selection state into the render object as its owner (which inverts the
widget/render split the rest of this file is built on).

## Corrected slicing

1. **(done, #960)** `RenderEditable` paints a selection it is told about.
2. **`TextEditingController` gains a selection.** Anchor + extent as source
   byte offsets, with every existing operation honouring it: `insert_str`
   replaces a non-collapsed range, `backspace`/`delete_forward` delete it
   rather than one char, the arrow moves collapse it to the appropriate end,
   `move_caret_home`/`end` collapse. One notification per mutation, as today.
   This is the unit that has to land next, and it is the one with real
   edge-case surface.
3. **Gestures drive it.** Tap places a collapsed selection, drag extends,
   and the widget maps between spaces (see the trap below). Needs
   `RenderEditable::byte_offset_for_local_offset` /
   `word_range_at_local_offset`, which are ~20 lines over
   `TextPainter::get_position_for_offset` / `get_word_boundary` and are
   deliberately NOT landed ahead of their caller — an unwired query is this
   repository's most common defect shape.

## The trap slice 3 must not walk into

`build_field_view` masks the text **before** it reaches the render object when
`obscure_text` is set. So every offset a pointer query returns is in MASKED
byte space while the controller holds SOURCE bytes.
`DEFAULT_OBSCURING_CHARACTER` is `U+2022` — three bytes — so the two spaces
diverge at the first character, and writing one into the other silently
corrupts the caret on any obscured field.

The inverse of `obscure`'s mapping is the same correspondence read backwards
(one mask char per source `char`): masked byte offset ÷ mask width gives a
char index, and that char's byte offset is the answer. It needs a test with a
multi-byte source character AND the multi-byte mask — a same-width fixture
proves nothing.

## Coordinate space, settled

`Listener`'s `PointerDispatch` carries both `local` (the listener's own box)
and `global` (the root's). Use `global` and map it into the editable's space
with `owner.transform_to(editable_id, root_id)` inverted, exactly as
`EditableTextState::global_caret_rect` maps the other way. Do **not** assume
the listener's local space equals the editable's: `AnchoredBox` sits between
them, and relying on proxies being zero-offset is an unstated coupling.

`global_caret_rect` is also the worked example for reaching the render object
at all: `inner_anchor.get()` → `owner.render_tree()` → first child →
`downcast_ref::<RenderEditable>()`, with the `PORT-CHECK-OK-DOWNCAST` marker
that grant requires. Note that inserting anything between `inner_anchor` and
the editable breaks that "first child" walk — which is why the `Listener`
belongs OUTSIDE the inner `AnchoredBox`.
