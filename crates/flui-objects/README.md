# flui-objects

**The concrete `RenderBox` / `RenderSliver` catalog for FLUI** — 74
ready-to-use render objects in six domain families, sitting directly above the
`flui-rendering` engine crate.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## Families

| Family | Contents |
|--------|----------|
| `layout` | sizing, alignment, flex, stack, transform, fitted-box, intrinsics, overflow, rotation, table |
| `proxy` | paint-effect proxies: opacity, clips, decoration, color, repaint boundary, leader/follower, shader mask |
| `interaction` | hit-test/visibility proxies: absorb/ignore pointer, offstage, mouse region, metadata |
| `text` | `RenderEditable`, `RenderParagraph` |
| `image` | `RenderImage` |
| `sliver` | the `RenderSliver*` family + `RenderViewport` |

All types re-export flat from the crate root: `flui_objects::RenderPadding`.

## Guarantees

- **Flutter parity by contract.** Each object ports the observable behavior
  (layout math, edge cases, hit-test order) of its Flutter counterpart;
  divergences are documented at the item.
- **Harness-tested.** Every exported object appears in the render-object
  test catalog (`RENDER_OBJECT_TYPES`) with `harness_*` tests exercising the
  real pipeline — the catalog completeness is CI-enforced.
- **Authoring-surface proof.** 74 objects compiling from outside
  `flui-rendering` prove the engine's custom-object authoring API is complete.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-objects --open`. The harness API is documented in
[`flui-rendering/docs/TESTING.md`](../flui-rendering/docs/TESTING.md).

## License

MIT OR Apache-2.0, per the workspace license.
