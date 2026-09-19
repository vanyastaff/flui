# ADR-0066: Display-list command representation

*A recorded command is `{ transform, op }`: the absolute transform it was
recorded under, and the operation. One state model, a two-cache-line op, a
copy-on-write path, and gradients as shader paints rather than a second
vocabulary.*

---

- **Status:** Accepted (landed 2026-09-18)
- **Date:** 2026-09-18
- **Deciders:** @vanyastaff
- **Scope:** `flui_painting::{DrawCommand, DrawOp, Canvas, DisplayList}`,
  `flui_types::painting::Path`, `flui_engine`'s dispatch and its painter
  surface. Follows [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md);
  amends nothing.

---

## Context

Four facts about the display list, each verified in the tree on 2026-09-18:

1. **Two state models at once.** Every variant carried a `transform: Matrix4`
   (absolute — the full CTM), *and* `Save`/`Restore` carried one too, so a
   reader could not tell whether the transform was per-command or scoped.
   `Canvas::draw_picture` had been deleted rather than fixed because
   re-stamping a picture meant editing a field in every one of 30 variants.
2. **560-byte commands.** `Path` was a 480-byte inline `SmallVec`, in three
   variants (`DrawPath`, `ClipPath`, `DrawShadow`); a `Vec<DrawCommand>` was
   dominated by empty path capacity, and `Path::clone` copied it. The Skia
   shape this transcribed (`SkPath` over a refcounted `SkPathRef`) is
   copy-on-write.
3. **Two gradient vocabularies.** `DrawGradient { rect, shader }` and
   `DrawGradientRRect { rrect, shader }` duplicated `DrawRect`/`DrawRRect`
   with a shader paint, on the *lesser* path: the engine's
   `render_gradient*` took no `Paint` (so no blend mode, no `anti_alias`),
   collapsed the rounded rect to one uniform radius, and skipped the
   painter's transform (it handed `bounds` to the batcher untransformed).
   `paint_box_decoration` was the only producer; the circle silhouette of
   the very same decoration already went through the shader-paint path.
4. **Serde on the wire.** `DrawCommand`/`DisplayList` derived serde with no
   consumer enabling it, and ADR-0065 put an `Arc<TextLayout>` on the wire
   that cannot be serialised. The derives had already come off in that
   change; this ADR records the decision.

## Decision

### The command is `{ transform, op }`

```rust
pub struct DrawCommand { pub transform: Matrix4, pub op: DrawOp }
pub enum DrawOp { Rect { rect, paint }, ClipRect { .. }, Paragraph { .. }, …, Save, Restore, SaveLayer { .. }, RestoreLayer }
```

`transform` is absolute (the full CTM at recording time), stamped once by
`Canvas::record`. Every command is self-describing: its bounds
(`op.local_bounds()` mapped through `transform`), a snapshot line, and a
re-stamp under another transform read one command without replaying state.
Clips are the one thing that scopes, and `Save`/`Restore` are *unit*
variants whose only job is to bracket them for the backend's clip stack.
`Canvas::draw_picture(&DisplayList)` is back as the one-liner the split
makes it: `ctm * command.transform` per command, ops cloned (paints and
paths are `Arc`s).

### The op has a budget

`size_of::<DrawOp>() ≤ 128` and `size_of::<DrawCommand>() ≤ 192`, pinned by
`draw_command_fits_its_budget`. The fattest variant is `ImageFiltered`,
whose inline `ColorFilter::Matrix` is 80 bytes; a variant that would exceed
the budget boxes its payload. `Path` is `Arc<Vec<PathCommand>>` with
`Arc::make_mut` on every mutator (32 bytes; a clone is a refcount bump;
`Path::shares_commands_with` observes the sharing).

### Gradients are shader paints

`DrawGradient`/`DrawGradientRRect`, `Canvas::draw_gradient*`,
`CommandRenderer::render_gradient*`, `WgpuPainter::draw_gradient_rect` /
`draw_radial_gradient_rect` / `draw_sweep_gradient_rect` /
`draw_shadow_rect` and the root re-exports of `GradientStop`/`ShadowParams`
are deleted. A gradient is `Paint::fill(..).with_shader(shader)` on
`Rect`/`RRect`/`Circle`, which reaches the engine's one shader-rect
dispatch (`dispatch_shader_rect`) with the paint's blend mode, its
`anti_alias`, the painter's transform, and the rounded rect's per-corner
radii. `paint_box_decoration` records one shader paint for all three
silhouettes.

### Serde is off the wire

`DrawCommand`, `DrawOp`, and `DisplayList` carry no serde derives. The wire
holds `Arc<TextLayout>` and `Arc<Paint>`; a serialisable scene, if one is
ever needed, is a separate projection, not the recording.

## Consequences

- A recorded scene is ~3× smaller per command (192 vs 560 bytes) and a path
  recorded from a caller's `Path` copies nothing. `display_list_record`
  (criterion, flui-painting) measures ~60 ns per recorded op and ~35 ns per
  replayed op on the reference machine.
- Gradient-filled rounded decorations now keep each corner's radius, honour
  the paint's blend mode and the painter's transform. That is a correctness
  fix, not only a deletion — `gradient_rrect_keeps_per_corner_radii` is red
  against a uniform radius (mutation-verified).
- `anti_alias` on a gradient background remains at `Paint`'s default, by
  choice rather than by capability limit: the reference's `isAntiAlias` is a
  `ColoredBox` parameter and a `ColoredBox` has no gradient (flui-painting
  `ARCHITECTURE.md` decision 7).
- The structural snapshot lines are unchanged: the op name and geometry
  print as before, and the `xf=[…]` suffix is appended once from
  `DrawCommand::transform`.
- `DrawCommand::Save`/`Restore` still carry a transform (every command
  does); nothing reads it, and the tests say so.

## Alternatives rejected

- **Scoped transforms (`Save` pushes, commands carry none).** Smaller
  commands, but every reader — bounds, damage, snapshot, `draw_picture` —
  becomes a replay with a matrix stack, and a single command is no longer
  self-describing. Skia's `SkPicture` playback is that model; FLUI's
  consumers are analysers as often as they are rasterisers.
- **Keep `DrawGradient*` and add a `Paint`.** Two vocabularies for one
  fill, with the engine keeping two lowering paths to drift apart.
- **`Box<Path>` instead of a COW buffer.** Same command size, but a clone
  still copies the commands, and `clip_path` on a caller's path is the
  common case.
- **A `Cow<'a, DisplayList>` for `draw_picture`.** A lifetime on the wire
  type for a call whose whole cost is one `Vec::extend`.

## Replacement tests

`draw_command_fits_its_budget` (`display_list/command.rs`);
`path_clone_shares_its_commands_until_mutated` (flui-types `path.rs`);
`draw_picture_restamps_by_the_current_transform` and
`save_and_restore_carry_no_state_but_the_clip_scope`
(`crates/flui-painting/tests/display_list_unit.rs`);
`box_decoration_gradient_records_a_shader_paint_rrect`
(`tests/decoration_unit.rs`); `gradient_rrect_keeps_per_corner_radii`
(engine readback, `gradient_blend_readback_tests.rs`). The reference has no
oracle for a display-list representation; `drawPicture` composition
(`canvas_test.dart`'s picture cases) is what the re-stamp test stands in
for.
