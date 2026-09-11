# Intent: update-only composited-layer commits for fragment-scope effects

- **Status:** Draft
- **Slug:** `996-fragment-scope-layer-updates`   ·   **Date:** `2026-09-10`   ·   **Stated by:** issue #996 (body and its 2026-09-08/10 comments); the maintainer's instruction in session was "Продолжай" after this was proposed as the next unit

## Asked for

> ### 3. Clip and physical model — need design, not wiring
>
> These are **not** node-level paint hooks like `paint_alpha()`/`paint_transform()`. They are recorded as *fragment scopes* (`FragmentOp::Push` with a `FragmentScope`) and replayed by the composer, so they are not covered by `own_effect_layers` and have no entry in `effect_slots`. Making them updatable means either promoting them to node-level hooks or teaching the capture to address fragment-scope layers by originating render id — a real design, not a setter change.
>
> Physical model additionally paints a shadow *and* clips, so an elevation change is not purely a layer property; check what actually lands on the layer versus in the display list before assuming it qualifies.

> (issue #996, last comment, "What is left on this issue") Only the fragment-scope design: clip rect/rrect/path, physical model, and backdrop/image filter all reach the frame as `FragmentScope`s replayed by the composer, never as `own_effect_layers` output, so none has an `effect_slots` entry. Either promote them to node-level hooks or teach the capture to address fragment-scope layers by originating render id. `paint_layer_blend` still has zero production overriders.

> (issue #996 body, "Measurement discipline this inherits") PR #994's benchmark carries two shapes on purpose, and any extension should be measured the same way: `inline` content merges into one shared `Arc<DisplayList>` and the update arm is flat (163× at 1000 nodes), while `layered` content makes a graft O(retained layers) and the win narrows to ~1.4×. Quoting only the inline number overstates the general case.

## What's wrong today

A property tick on a clip, a physical model, or a backdrop filter — a reveal animation driving a `ClipPath`'s clipper, a blur-in driving a backdrop filter's sigma, an elevation animation on a Material surface — repaints the whole subtree under the nearest repaint boundary every frame, because the effect reaches the frame as a fragment scope the paint walk replays, and a `COMPOSITED_LAYER_UPDATE` mark on such a node is inert: `layer_patches_for` finds no effect slot for it and the enclosing boundary repaints in full. Opacity, transform, and rotated box already get the cheap path (a patched layer, subtree not repainted, 133–208× at 1000 inline nodes); the fragment-scope effects do not, and nothing tells a widget author which of the two classes they are animating.

## What "fixed" looks like

Ticking the property of a fragment-scope effect under a retained boundary leaves the subtree's paint count flat and produces the same layer tree a full repaint would — for every producer that can be served without a structural change; for the ones that cannot (or are not worth it), the record says so and why, and their setters still report the repaint they need.

## Who feels it

Framework users animating clips, blurs, and elevations in FLUI apps (frame cost); the next maintainer reading `ARCHITECTURE.md`'s composited-layer-update entry to learn which effects are patchable; the #996 issue, which stays open until this is decided one way or the other.

## Constraints the user owns

- Flutter 3.44.0 is the floor: none of `_RenderCustomClip`, `RenderPhysicalModel`, `RenderPhysicalShape` are repaint boundaries or override `updateCompositedLayer` there (setters `markNeedsPaint`), so serving them is an improvement that owes an `ARCHITECTURE.md` `## Mapping decisions` entry and a replacement test per producer; `_ImageFilterRenderObject` IS an overrider upstream (`enabled` → `markNeedsPaint`, `imageFilter` → `markNeedsCompositedLayerUpdate`) and that split is to be ported as-is where FLUI's surface supports it.
- Every extension is measured on the benchmark's two shapes (`inline` and `layered`), both arms dirtying the same node, and the record quotes both numbers.
- The captured-origin invariant and the per-position structure guard of `effect_slots` are not to be weakened; a structural transition (clip appears/disappears, filter `enabled` flips, a shadow changes in the display list) reports `PAINT`.
- `RenderFittedBox`'s recorded asymmetry (its transform stays inside `paint` so its clip can open outside the transform layer) is a constraint a design must express, not a defect to normalise.

## Not this

- Not promoting any node to a repaint boundary (the flat-capture argument stands).
- Not changing what any effect draws; a frame produced by the patch path must equal the frame a full repaint produces.
- Not the semantics or hit-testing of clips.
- Not `paint_layer_blend`'s consumer story beyond deciding whether that dead hook stays, goes, or is repurposed by the chosen design.

---

## User amendments

- **2026-09-10, in-session message (after the orchestrator proposed this unit as next):**
  > Продолжай

## Corrections

| Date | What changed | Why it surfaced only now |
|------|--------------|--------------------------|
| — | — | — |
