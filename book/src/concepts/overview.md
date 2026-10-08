# Concepts

FLUI uses declarative Rust values to describe UI. A new description does not mean throwing away
all the UI's state: reconciliation matches it to retained elements, which own lifecycle and
state. Render objects then lay out and paint the result.

```text
View → Element → RenderObject → Layer → flui-engine → wgpu
                    │
                    └→ Semantics
```

Each part answers a different question:

| Part | Responsibility | Lifetime |
|---|---|---|
| `View` | What UI should this build describe? | Immutable configuration replaced during rebuilds. |
| `Element` | Which existing UI identity and state does that description match? | Retained while reconciliation preserves its identity. |
| `RenderObject` | What size, paint output and hit-test behavior does it produce? | Retained and updated by its owning element. |
| `Layer` | How should painted output be composited? | A fresh layer tree is built for each frame. |
| `Semantics` | What roles, properties and actions should accessibility expose? | A separate tree alongside the visual pipeline. |

A view that composes other views need not own a render object. Rebuilding a view, laying out a
render object and repainting it are separate operations; a change can invalidate the work it
needs without recreating the entire pipeline.

## Read the concepts in order

- [View, Element, RenderObject](view-element-render.md) — the trees themselves.
- [Keys](keys.md) — identity and reparenting.
- [Lifecycle](lifecycle.md) — the states an `Element` moves through.
- [Layout: constraints down, sizes up](layout.md) — the layout protocol.
- [State: signals, local state, and shared data](state.md) — state handles, inherited data,
  notifiers and presentation-scoped signals.

## From an interaction to a frame

An event callback changes state and requests the affected work. During build, views read state
and describe children; reconciliation preserves matching elements. Layout passes constraints
down and sizes up, then paint records output for composition. The engine consumes the scene
and submits GPU work. The platform and scheduler arrange frames on demand.

Build, layout and paint run synchronously. Async work belongs at the IO and scheduling edges;
its results enter UI work through the runtime's delivery paths. Presentation capabilities such
as focus and rebuild handles are acquired in lifecycle hooks, which receive
`LifecycleContext`; `build` receives `BuildContext` for describing UI and reading dependencies.

For ownership, frame transactions and crate boundaries, continue with the
[architecture guide](../architecture.md). The [Flutter → FLUI mapping](../mapping.md) can help
translate familiar vocabulary; FLUI's contracts and Rust APIs determine the behavior here.
