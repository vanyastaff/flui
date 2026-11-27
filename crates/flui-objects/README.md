# flui-objects

**The concrete `RenderBox` / `RenderSliver` catalog for FLUI** — ready-to-use
render objects in six domain families, sitting directly above the
`flui-rendering` engine crate. `RENDER_OBJECT_TYPES` in
[`tests/render_object_harness.rs`](tests/render_object_harness.rs) lists them.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## Families

| Family | Contents |
|--------|----------|
| `layout` | sizing, padding, alignment, baseline, aspect ratio, flex, wrap, flow, list body, stack, indexed stack, theater, table, transform, fractional translation, fitted box, intrinsics, overflow and limits, rotation, custom layout delegates, layout builder, container, toolbar items |
| `proxy` | paint-effect and annotation proxies: opacity, clips, decoration, color, custom paint, backdrop filter, shader mask, physical model, repaint boundary, leader/follower, paint and subtree anchors (`RenderSliverSubtreeAnchor` among them), semantics annotations, annotated region, visibility, skeleton and shimmer mask, ignore baseline, error box |
| `interaction` | hit-test/visibility proxies: absorb/ignore pointer, listener, mouse region, hit slop, content shape, input region, interactive, press pass-through, offstage, metadata |
| `text` | `RenderEditable`, `RenderIcon`, `RenderParagraph` |
| `image` | `RenderImage`, `RenderTexture` |
| `sliver` | the `RenderSliver*` objects other than `RenderSliverSubtreeAnchor` (in `proxy`), `RenderViewport` and `RenderShrinkWrappingViewport` |

All types re-export flat from the crate root: `flui_objects::RenderPadding`.

## Guarantees

- **Edge cases are tested.** Each object handles its edge cases (layout math, hit-test
  order) behind a Rust-shaped API.
- **Harness-tested.** Every exported object appears in the render-object
  test catalog (`RENDER_OBJECT_TYPES`) with a `harness_*` row in a family table test exercising the
  real pipeline — the catalog completeness is CI-enforced.
- **Authoring-surface proof.** Every object in the catalog compiles from outside
  `flui-rendering`, which proves the engine's custom-object authoring API is complete.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-objects --open`. Architectural mapping decisions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md). The harness API is documented in
[`flui-rendering/docs/TESTING.md`](../flui-rendering/docs/TESTING.md).

## License

MIT OR Apache-2.0, per the workspace license.
