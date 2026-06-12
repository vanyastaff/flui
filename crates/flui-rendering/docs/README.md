# flui_rendering documentation

Crate-local guides for the render pipeline, protocols, and test harness.

## System guides

| Document | Topic |
|----------|-------|
| [PROTOCOL_ARCHITECTURE.md](./PROTOCOL_ARCHITECTURE.md) | Box / Sliver protocols and capability traits |
| [LAYOUT_SYSTEM.md](./LAYOUT_SYSTEM.md) | Layout pipeline and constraints |
| [PAINT_SYSTEM.md](./PAINT_SYSTEM.md) | Paint and compositing |
| [HIT_TEST_SYSTEM.md](./HIT_TEST_SYSTEM.md) | Hit testing |
| [ROADMAP.md](./ROADMAP.md) | Crate-level construction notes |

## Testing

| Document | Topic |
|----------|-------|
| **[TESTING.md](./TESTING.md)** | `RenderTester` harness — API, multi-frame animation, examples |
| [render_inspector example](../examples/render_inspector.rs) | Runnable headless inspector |
| [render_object_harness.rs](../tests/render_object_harness.rs) | CI catalog of all render types |

## Related harness docs

- [flui-layer/docs/TESTING.md](../../flui-layer/docs/TESTING.md) — layer-tree builders and walkers
- [flui-painting/docs/TESTING.md](../../flui-painting/docs/TESTING.md) — display-list recording
- [flui-foundation/docs/TESTING.md](../../flui-foundation/docs/TESTING.md) — diagnostics for assertions
- [Workspace testing guide](../../../docs/testing.md) — CI commands and conventions
