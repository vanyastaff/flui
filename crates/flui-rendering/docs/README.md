# flui_rendering documentation

Crate-local guides for the render pipeline, protocols, and test harness.

## System guides

| Document | Topic |
|----------|-------|
| [LAYOUT_SYSTEM.md](./LAYOUT_SYSTEM.md) | Layout pipeline and constraints |
| [HIT_TEST_SYSTEM.md](./HIT_TEST_SYSTEM.md) | Hit testing |

## Testing

| Document | Topic |
|----------|-------|
| **[TESTING.md](./TESTING.md)** | `RenderTester` harness — API, multi-frame animation, examples |
| [render_inspector example](../examples/render_inspector.rs) | Runnable headless inspector |
| [render_object_harness.rs](../../flui-objects/tests/render_object_harness.rs) | CI catalog of all render types |

## Related harness docs

- [flui-layer/README.md](../../flui-layer/README.md) — layer-tree construction and the `testing::inspect` walkers
- [flui-painting/src/testing/mod.rs](../../flui-painting/src/testing/mod.rs) — display-list recording
- [flui-foundation/docs/TESTING.md](../../flui-foundation/docs/TESTING.md) — diagnostics for assertions
- [Workspace testing guide](../../../docs/testing.md) — CI commands and conventions
