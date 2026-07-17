# ADR-0033: Composing-region rendering — underline, hidden caret, composing-rect cursor area

*The IME composing region now paints a thin underline and can hide the caret while the platform owns its position; the cursor-area loop prefers the composing region's rect over the collapsed caret's, matching Flutter's own fallback order.*

---

- **Status:** Accepted
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-widgets/src/text/controller.rs` (`ComposingState`, `TextEditingController::caret_hidden_by_ime`, the `Preedit("")`-cancels-composition fix); `crates/flui-objects/src/text/editable.rs` (`RenderEditable::composing_range`, `set_composing_range`, `rect_for_composing_range`, the underline paint path); `crates/flui-widgets/src/text/editable_text.rs` (`EditableTextRenderView::composing_range`, `build_field_view`'s gating, `CursorAreaLoop::global_caret_rect`'s fallback order); `crates/flui-types/src/ime.rs` (doc clarification only)
- **Related:** ADR-0030 (`ImeEvent`/`TextEditingController` composing model — this ADR amends its hidden-caret deferral and fixes an empty-preedit contract divergence); ADR-0032 (the cursor-area loop this ADR upgrades from caret-rect-only to composing-rect-preferred)

---

## Context

ADR-0030 (PR2) landed `TextEditingController`'s composing-region model and named two things as deferred, not implemented: a visual underline over the composing text, and hiding the caret while the IME owns its position (winit's `Preedit { cursor: None }` signal). ADR-0032 then wired the IME cursor-area loop to the collapsed caret's rect only, also naming the composing-rect upgrade (Flutter's own `_updateComposingRectIfNeeded` fallback order) as deferred.

Both deferrals had the same root cause: `RenderEditable` had no rendering state for the composing region beyond the range `TextEditingController` already tracked, and the controller's own hidden-caret handling collapsed the caret to the end of the region instead of tracking a separate "hidden" bit — a decision that was itself a stated compromise ("a flag with no consumer would be a lie of completeness").

Auditing the fix surfaced a **pre-existing bug**, not a new one this ADR introduces: winit signals a *cancelled* composition as `Preedit { text: "", cursor: None }`, with no following `Commit`/`Disabled` event. `TextEditingController::set_composing_text` treated this identically to any other preedit update — it stripped the (now-empty) composing slice from the buffer, correctly, but then set `composing = Some(n..n)`, an empty-but-active region. `is_composing()` therefore stayed `true` forever after a cancelled composition, permanently suppressing `Key::Character` insertion for the rest of the focus session — exactly the failure mode `ImeEvent`'s own suppression-contract doc warns against, just never triggered because nothing exercised the empty-preedit path. This ADR fixes it as part of the same reshape, not a separate change, because the fix and the underline/hidden-caret work touch the identical `composing` field.

## Decision

### 1. `ComposingState` replaces a bare `Option<Range<usize>>`

`ControllerInner::composing` becomes `Option<ComposingState>` where `ComposingState { range: Range<usize>, caret_hidden: bool }`, instead of adding a sibling `caret_hidden: bool` field directly on `ControllerInner` alongside the range.

**Why one option, not two fields.** A sibling-bool shape is structurally leak-prone: nothing except programmer discipline stops `caret_hidden` from staying `true` after composition ends — every single site that clears `composing` (`commit_text`, `clear_composing`, every non-IME mutator, and the new empty-preedit-cancel path below) would *also* have to remember to clear `caret_hidden`, a rule enforced by convention, not the type system. Folding both fields into one `Option<ComposingState>` makes the leak impossible instead of merely disciplined: `composing = None` at any site drops `caret_hidden` for free, because there is no longer a `caret_hidden` slot independent of `composing` to forget about.

`composing_range()`'s public signature is unchanged (`Option<Range<usize>>`, mapped from the new internal shape); `is_composing()`'s semantics are unchanged except for the empty-preedit fix below. The new accessor `caret_hidden_by_ime() -> bool` returns `false` whenever no composition is active, so a caller never has to check `is_composing()` first.

### 2. `Preedit("")` ends composition — the empty-preedit bug fix

`set_composing_text` now branches on `text.is_empty()`: an empty preedit strips the existing composing slice (same replace-with-empty operation as before) but sets `composing = None`, not `Some(empty range)`. Composition ends; `is_composing()` returns `false`; plain typing works immediately. A regression test (`empty_preedit_ends_composition_instead_of_leaving_an_empty_active_region`, `controller.rs`) pins the full cycle: `Preedit("nihao") → Preedit("") → is_composing() == false`, then a typed character actually reaches the buffer.

### 3. Caret-navigation restores caret visibility without ending composition

Direct caret movement (`move_caret_left`/`right`/`home`/`end`) clears `caret_hidden` back to `false` whenever a composition is active, leaving the composing `range` itself untouched — the user took the caret back, so the IME no longer owns its position, but the composition (and its underline) keeps running. This preserves the existing pinned test `clear_composing_leaves_a_caret_before_the_region_untouched` unchanged (range semantics didn't move) while adding the visibility flip as an orthogonal effect.

### 4. The composing-region underline — a declared 1px approximation, not font metrics

`RenderEditable` gains `composing_range: Option<Range<usize>>` (builder `with_composing_range`, mutator `set_composing_range` returning `Invalidation::Paint` — paint-only, since the composing region never changes glyph shaping) and paints one thin rect per selection box under it in `paint()`, using the already-cached layout's `get_boxes_for_selection` (byte-indexed, `&self`, no re-shape).

**This is explicitly not font underline metrics.** `TextStyle` has no `decoration` field — Flutter's `TextEditingController.buildTextSpan` merges `TextStyle(decoration: TextDecoration.underline)` into the composing span's style and lets its text-layout engine (Skia) render the actual underline stroke from font metrics; FLUI's `TextStyle` carries no decoration concept at all today, so there is nothing to merge into. Instead, `RenderEditable` paints a flat 1-logical-pixel-thick rect positioned 1px below the alphabetic baseline (`compute_distance_to_actual_baseline`), clamped to stay inside each selection box's vertical span. This is a **declared divergence**, not parity: do not describe this as matching Flutter's rendered underline weight, position, or style (single vs. double, dashed, etc.) — it is a visual approximation that communicates "this text is being composed," nothing more.

**Migration path.** `TextDecoration`/underline styling is presently an orphaned concept with no home — `TextStyle` doesn't carry it, so this feature had to paint the underline as a bespoke rect rather than as ordinary styled text. When span-level decoration lands as a general text feature (a `TextStyle.decoration` field consumed by a `buildTextSpan`-equivalent), the composing underline should move onto that mechanism and this bespoke paint-rect path retires — the two must not coexist as separate underline implementations.

**Color resolution.** The underline must paint in the same color as the glyphs it sits under, by construction. `RenderEditable` cannot see the color the rendering engine ultimately resolves (`flui-engine`'s `render_text_span` computes `root_style.and_then(|s| s.foreground.or(s.color)).unwrap_or(Color::BLACK)` deep in the `wgpu` backend, several layers past what `RenderEditable` has access to) — but `RenderEditable` already owns the same `TextStyle` that resolution reads, through `painter().text()`. So `resolved_glyph_color` recomputes the identical formula (`foreground.or(color).unwrap_or(BLACK)`) locally rather than requiring a separately plumbed color parameter from the widget layer — one source of truth for the resolution rule, read at two different layers of the same pipeline. Two harness tests pin this: default style (falls back to black) and an explicit color (matches exactly), both asserting the underline's paint-op color against the same resolved value.

**Multiline is out of scope, not silently broken.** `get_boxes_for_range` (the layout-cache method `get_boxes_for_selection` delegates to) compares a *global byte range* against *per-run glyph byte indices* gathered while walking every laid-out line. This is correct today only because `RenderEditable` is hard-constrained to `max_lines(1)` — a single line means a single set of glyph runs with no cross-line byte-range ambiguity. When multiline lands, this comparison must be revisited (byte ranges spanning multiple lines, or a range that lands entirely on a non-first line, are unverified against this method's actual per-line accumulation behavior).

`rect_for_composing_range()` folds the boxes via `Rect::union` into one bounding rect for the cursor-area loop (below). It returns `None` — never `Rect::ZERO` — whenever there is nothing meaningful to report: no active range, an empty range, no layout yet, or zero boxes. A `Rect::ZERO` reaching a caller (especially the platform, via the cursor-area loop) would read as "the composing region is at the origin," a false positive; `None` is the only honest signal for "there is no composing region."

### 5. Hidden caret while composing — realized at the widget layer, not a new render-object state

`RenderEditable` gains no new "hidden because composing" rendering concept — it still only has the one `show_caret: bool` flag it always had. `EditableTextRenderView::build_field_view` computes `show_caret: focused && !controller.caret_hidden_by_ime()`, so the existing flag does the work. This mirrors the underline's own gating: `composing_range` is passed to the render view **only** when `enabled && has_primary_focus()` — the FLUI analog of Flutter's `_EditableTextState.buildTextSpan`'s `withComposing: !widget.readOnly && _hasFocus` (`editable_text.dart`, tag `3.44.0`; FLUI has no `readOnly` field, so this substrate reuses the existing `enabled` hoist named in `EditableText::enabled`'s own doc). An unfocused field must not keep painting a stale composing underline for input it no longer owns, even though blur alone does not end the composition (only detaches the IME client — the composition itself survives a blur/refocus cycle, per existing `TextInputRegistry` semantics).

The ADR-0032 regression guard stays intact: `caret_local_rect` remains visibility-independent (`show_caret` gates only `paint`, never the geometry accessor) — the existing pinned harness test for this is unchanged and still green.

### 6. Cursor-area loop: composing rect preferred, caret rect as fallback

`CursorAreaLoop::global_caret_rect` (unrenamed — its doc is updated, the identifier stays, since `TextInputHandle::set_cursor_area`'s semantics are "the rect the candidate window should track," which is the caret only in the no-composition case) now computes:

```rust
let local_rect = editable
    .rect_for_composing_range()
    .unwrap_or_else(|| editable.caret_local_rect());
```

This is Flutter's own `_updateComposingRectIfNeeded` order: prefer the composing rect, fall back to the caret rect when none is available. The existing per-attach alive-flag/dedupe-cache machinery (ADR-0032) is untouched — only the rect *source* changed, not the scheduling or send-dedupe logic around it.

## Consequences

- Composing text is now visually distinguishable from committed text (a real, if approximate, IME affordance) and the caret correctly disappears while the platform IME owns its position — closing the two deferrals ADR-0030 named.
- The empty-preedit-cancel bug fix is a correctness fix that predates this ADR's feature work but ships in the same change, since both touch `ComposingState`.
- The underline is an explicit visual approximation (1px flat bar, not font metrics) until `TextStyle` grows a `decoration` field — documented here and in `RenderEditable`'s module doc so no future reader mistakes it for parity.
- The composing-rect/caret-rect fallback is single-line only, tracking `RenderEditable`'s own `max_lines(1)` constraint; multiline support must revisit `get_boxes_for_range`'s glyph-index-vs-global-byte-range comparison before this fallback can be trusted across line boundaries.

## Amends

- **ADR-0030**: closes the "hidden caret while composing" named deferral; fixes the empty-preedit (`Preedit("")`) contract divergence in `TextEditingController::set_composing_text` (a latent bug in the original composing model, not a new behavior change).
- **ADR-0032**: upgrades the cursor-area loop's sanctioned caret-only fallback to prefer the composing-region rect, per Flutter's own `_updateComposingRectIfNeeded` order.

## Evidence

- `cargo nextest run --workspace --exclude flui-platform`, `cargo test -p flui-platform --lib`: all passing (paste exact counts from the PR's gate run).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `just fmt-check port-check inventory-check`, `taplo fmt --check`, `typos`: clean.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-types -p flui-objects -p flui-widgets -p flui-app --no-deps --document-private-items`: clean.
- Harness catalog guard (`cargo test -p flui-objects --test render_object_harness`): `RenderEditable`'s row covers the new underline/composing-rect behavior with dedicated `harness_editable_*` tests, including a multibyte (CJK) byte-offset agreement pin.
- Red→green evidence for every load-bearing behavior this ADR introduces (mutation-verified, not merely executed once):
  - `empty_preedit_ends_composition_instead_of_leaving_an_empty_active_region` — reverting the `text.is_empty()` branch reproduces the permanent-suppression bug.
  - `caret_navigation_restores_the_caret_while_composing` (+ its widget-level counterpart through the real key handler) — dropping the `clear_caret_hidden` call leaves the caret hidden after direct navigation.
  - `harness_editable_composing_underline_paints_at_the_exact_multibyte_box` — no underline paint code, or a char-offset/byte-offset mixup, fails this test.
  - `harness_editable_rect_for_composing_range_none_cases` — returning `Some(Rect::ZERO)` for an empty range fails the assertion.
  - `unfocus_mid_composition_stops_passing_the_composing_range` — removing the `focused` gate around `composing_range` in `build_field_view` fails this test.

## What is deferred

- The underline-as-bespoke-rect approach retires once `TextStyle` gains a `decoration` field and a `buildTextSpan`-equivalent exists to merge it — see "Migration path" above.
- Multiline composing-region geometry — `get_boxes_for_range`'s per-line glyph-index comparison is unverified past `max_lines(1)`.
- Real-IME manual verification (ibus/fcitx/platform IME) of the underline and hidden-caret behavior on a live composition session remains the same named manual-verification gap ADR-0030/ADR-0032 already carry — this ADR is machine-verified headlessly only.
