# AGENTS.md — flui-painting

Backend-agnostic Canvas API. Records drawing commands into an immutable `DisplayList` for later GPU execution.

## What lives here

- `Canvas` — main drawing interface with save/restore state stack
- `DisplayList` — immutable sequence of recorded `DrawCommand`s
- `Paint` — styling (color, stroke, shader, blend mode)
- Text shaping via `cosmic-text`

## Architecture

```
RenderObject (flui-rendering) → Canvas API (this crate) → DisplayList → WgpuPainter (flui-engine) → GPU
```

## Key constraints

- `#[forbid(unsafe_code)]` — no unsafe in this crate
- No `RwLock<Box<dyn RenderObject>>` — enforced by port-check trigger #1
- `testing` feature enables `crate::testing` module (declarative DisplayList builder for tests)
- Self dev-dependency pattern: `flui-painting = { path = ".", features = ["testing"] }`

## Architecture doc

See `crates/flui-painting/ARCHITECTURE.md` for deep architecture.
