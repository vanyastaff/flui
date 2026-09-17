QUESTION: Do public DisplayList opacity/transform operations preserve semantics through deeply nested effect commands?

ANSWER: No. `DisplayList::to_opacity` and `DisplayList::apply_transform` preserve a 63/64-deep public `Canvas::draw_shader_mask` nest, but at depth 65 the deepest draw command is left unchanged because `MAX_EFFECT_DEPTH` saturates and the child display list is not visited.

VERSIONS: flui workspace at HEAD b482a20824ba80c961a5c5074e2df6a8880f8b1e as inspected on 2026-09-13.

SOURCES:
- `crates/flui-painting/src/display_list/command_ops.rs:15-43` documents `MAX_EFFECT_DEPTH = 64` and says visual fidelity degrades for deeper effect stacks.
- `crates/flui-painting/src/display_list/command_ops.rs:347-373` returns an unchanged clone when `with_opacity_depth` saturates.
- `crates/flui-painting/src/display_list/command_ops.rs:879-891` returns without visiting nested child display lists when `apply_transform_depth` saturates.
- `crates/flui-painting/src/canvas/drawing.rs:448-490` exposes public `draw_shader_mask` / `draw_backdrop_filter` recorders.
- `crates/flui-painting/src/canvas/composition.rs:104-118` uses `apply_transform` when replaying cached display lists at a non-zero offset.
- Scratch probe: `/mnt/data/audits/rendering-g-20260913/display-depth-probe`.
- Probe output: `depth=63 alpha=Some(0.49803922) tx=Some(9.0)`, `depth=64 alpha=Some(0.49803922) tx=Some(9.0)`, `depth=65 alpha=Some(1.0) tx=Some(0.0)`.

OPEN: Whether FLUI wants to replace the cap with an iterative traversal or keep an explicit public depth limit is a design decision. The current behavior is filed as #1084 because silent partial success is the concrete problem.

ANSWERED
