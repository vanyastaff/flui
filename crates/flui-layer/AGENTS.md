# AGENTS.md — flui-layer

Compositor layer tree — the fourth tree in FLUI's 5-tree architecture (View → Element → Render → Layer → Semantics).

## What lives here

- Layer types: `CanvasLayer`, `TextureLayer`, `PlatformViewLayer`
- Clip layers: `ClipRectLayer`, `ClipRRectLayer`, `ClipSuperellipseLayer`, `ClipPathLayer`
- Transform layers: `OffsetLayer`, `TransformLayer`
- Effect layers: `OpacityLayer`, `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer`
- Linking layers: `LeaderLayer`, `FollowerLayer`
- `LayerTree` — implements `TreeRead<LayerId>` + `TreeNav<LayerId>` from `flui-tree`
- `Scene` — top-level rendering unit (size + layer tree + root + DPR)

## Key constraints

- No `RwLock<Box<dyn Layer/ContainerLayer>>` — enforced by port-check trigger #1
- No `async fn` in `composite`/`render`/`fire_composition_callbacks` — enforced by trigger #3
- `testing` feature enables declarative LayerTree builder + diagnostics dump
- Self dev-dependency pattern for integration tests

## Architecture doc

See `crates/flui-layer/ARCHITECTURE.md` for deep architecture.

## Note

This crate's `Cargo.toml` still uses `edition = "2021"` — predates workspace standardization.
