---
date: 2026-05-20
topic: flui-painting allocation hot-path audit
applies-to: crates/flui-painting/
origin: Mythos chain Step 9
---

# flui-painting Allocation Hot-Path Audit

Documentation-only artifact produced during Mythos chain Step 9. Identifies the per-`Canvas::draw_*` allocation pattern in `crates/flui-painting/` and files measured-optimisation deferrals as Outstanding refactors per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception. No code changes are made in U9; this audit feeds into U12's `crates/flui-painting/ARCHITECTURE.md` `## Friction log` and `## Outstanding refactors` sections.

---

## Methodology

Static read of `crates/flui-painting/src/canvas/drawing.rs` (post-U4 split, 456 LOC) + cross-reference with `crates/flui-painting/src/display_list/command.rs` (post-U5 split, ~433 LOC) + cross-reference with `crates/flui-painting/src/display_list/command_ops.rs`. No runtime profiling; pure structural analysis of what each `Canvas::draw_*` call site emits.

A synthetic 1,000-`draw_rect`-call workload was the mental model. For a realistic 60 fps frame budget (16.67 ms total) with ~500-2000 commands per frame, allocation churn matters.

---

## Findings

### F1. `Paint.clone()` per `Canvas::draw_*` call

**Site.** Every primary `draw_*` method in `canvas/drawing.rs` (and the optional-paint variants) calls `paint.clone()` to bake the `Paint` into the emitted `DrawCommand`:

```rust
// canvas/drawing.rs:57
pub fn draw_rect(&mut self, rect: Rect<Pixels>, paint: &Paint) {
    self.display_list.push(DrawCommand::DrawRect {
        rect,
        paint: paint.clone(),   // <- allocation hot spot
        transform: self.transform,
    });
}
```

**Cost.** `Paint` is ~80-200 bytes including the optional `Box<Shader>` payload. For 1,000 `draw_rect` calls with a single reused `Paint`, this is 1,000 × `Paint::clone()` = ~80-200 KB of redundant cloning per frame.

**Affected methods.** All 29 primary `draw_*` methods. The `draw_image*` variants take `Option<&Paint>` and use `paint.cloned()` (an additional `Option::map(Clone::clone)`).

**Proposed fix.** Paint interning at construction:
- Canvas owns a `Vec<Paint>` interning table.
- `DrawCommand` variants carry `PaintHandle(NonZeroU32)` instead of `Paint`.
- Engine resolves `PaintHandle` to `&Paint` at GPU lowering time.

**Named blockers.**
- `Paint: Hash + Eq` -- Paint contains `f32` color components; not `Eq`. Requires either (a) a `derive(Hash)` impl that does bit-pattern hashing of f32, (b) wrapping each f32 in `OrderedFloat` from the `ordered-float` crate, or (c) interning at the `Color` granularity instead.
- Per-canvas interning table -- new state on `Canvas` struct; must be cleared on `reset()`.
- Engine-side handle resolution -- `flui-engine`'s wgpu backend must accept the handle + table on the `Scene`/`DisplayList` boundary.
- Measured benchmark -- `criterion` bench harness for a 1,000-`draw_rect` synthetic workload showing the wall-clock improvement before/after.

**Verdict.** Real optimisation; defer to Outstanding.

### F2. Per-`DrawCommand` 64-byte `Matrix4` baking

**Site.** Every `DrawCommand` variant stores its own `Matrix4` (16 × 4 bytes = 64 bytes) baked from `self.transform` at recording time:

```rust
// display_list/command.rs ~85
DrawLine {
    p1: Point<Pixels>,
    p2: Point<Pixels>,
    paint: Paint,
    transform: Matrix4,   // <- 64 bytes per command
},
```

**Cost.** For 1,000 commands per frame, 64 KB of matrix data is baked into the `Vec<DrawCommand>`. If the transform is invariant across most commands (typical for a Row/Column with a flat translate stack), this is redundant.

**Affected variants.** All 29.

**Proposed fix.** A flat-bytecode `Vec<u8>` `DisplayList` representation (Skia's `SkRecord` shape):
- Replace `Vec<DrawCommand>` with a byte buffer + opcode tags.
- The engine decodes opcode + payload at GPU lowering.
- Transforms can be represented as separate "set transform" opcodes between draw operations, dedup-shared across runs.

**Named blockers.**
- Bytecode encoder per `DrawCommand` variant.
- Bytecode decoder on the engine side.
- Re-shape `DrawCommand::with_opacity`, `apply_transform`, `bounds`, `filter`, `map`, `to_opacity` operations to work over bytecode (significant refactor of `display_list/command_ops.rs`).
- Loss of `serde` derive ergonomics on `DrawCommand` (the byte buffer would need its own `Serialize`/`Deserialize`).
- Measured benchmark showing meaningful improvement over flat `Vec<DrawCommand>`.

**Verdict.** Real optimisation but very high blast radius; defer to Outstanding.

### F3. `Path.clone()` per `draw_path` / `draw_shadow` call

**Site.** `Canvas::draw_path` and `Canvas::draw_shadow` clone the `Path` argument:

```rust
// canvas/drawing.rs:107
pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
    self.display_list.push(DrawCommand::DrawPath {
        path: path.clone(),   // <- Vec<PathCommand> heap allocation
        paint: paint.clone(),
        transform: self.transform,
    });
}
```

**Cost.** `Path` interior is `Vec<PathCommand>` (heap-allocated). For a path with N commands, `Path::clone()` is O(N) + one heap allocation. For repeated `draw_path` calls with the same path (typical for repeated UI elements like icons), this is wasted work.

**Affected methods.** `Canvas::draw_path`, `Canvas::draw_shadow`, `Canvas::clip_path`, `Canvas::clip_path_ext`. The `clip_path*` variants ADDITIONALLY do `Box::new(Path::clone())` to wrap the path for `ClipShape::Path` variant uniformity in the clip stack:

```rust
// canvas/clipping.rs:43
pub fn clip_path(&mut self, path: &Path) {
    self.clip_stack
        .push(ClipShape::Path(Box::new((*path).clone())));  // 2 allocations
    // ...
}
```

**Proposed fix.** Path Clone-on-Write semantics:
- `Path` interior changes from `Vec<PathCommand>` to `Arc<[PathCommand]>` or `Rc<Vec<PathCommand>>`.
- `Path::clone()` becomes `Arc::clone()` -- one atomic increment, no heap allocation.
- `Path::push(...)` (if mutation is needed) does `Arc::make_mut`.

**Named blockers.**
- `Path` lives in `flui-types::painting`. Requires a `flui-types` breaking change.
- Existing `Path::push` / `Path::move_to` etc. callers compile unchanged via `Arc::make_mut` but pay one-time cost on first mutation.
- Measured benchmark showing benefit on realistic workloads (e.g. a UI with N repeating icon paths).

**Verdict.** Real optimisation; defer to Outstanding under `flui-types` breaking change blocker.

### F4. `Box::new(Path::clone())` for `ClipShape::Path` variant uniformity

**Site.** `Canvas::clip_path*` methods wrap the cloned path in `Box` for variant size uniformity:

```rust
self.clip_stack.push(ClipShape::Path(Box::new((*path).clone())));
```

**Cost.** Two allocations per `clip_path` call: the inner `Path::clone()` heap alloc + the outer `Box::new()` heap alloc.

**Proposed fix.** Either (a) inline `Path` directly in `ClipShape::Path(Path)` and accept the variant-size inflation (probably fine since `Path` is a `Vec` header = 24 bytes; the largest variant is already `Rect` at ~16 bytes or `RRect` at ~32 bytes; difference negligible), or (b) the F3 Path-Cow refactor naturally eliminates the per-clip allocation.

**Named blockers.** None for option (a); see F3 for option (b).

**Verdict.** Small win; bundle with F3 in Outstanding.

### F5. `cosmic-text` font shaping lock contention

**Site.** `crates/flui-painting/src/text_layout/layout.rs::font_system()` returns `&'static Mutex<FontSystem>`. Every `TextLayout::new` call locks the mutex for the duration of `Buffer::set_text` + `Buffer::shape_until_scroll`.

**Cost.** cosmic-text 0.12 `Buffer::shape_until_scroll` is synchronous and may take 1-10 ms for complex text. Multiple text widgets shaping simultaneously serialise on the global mutex.

**Affected.** `TextLayout::new`, `TextPainter::layout` (transitively), `measure_text`, `measure_inline_span`.

**Proposed fix.** Per-thread `FontSystem` instances via cosmic-text 0.13+ (which supports thread-local font systems).

**Named blockers.**
- cosmic-text 0.12 → 0.13+ upgrade. API surface changes may ripple into `text_layout/*` and `text_painter/measure.rs`.
- Measured benchmark showing concurrent text-shape workloads benefit.

**Verdict.** Real optimisation under cosmic-text upgrade blocker; defer to Outstanding.

### F6. `tracing::instrument` macro overhead on per-command path -- NOT a concern

**Site.** No `tracing::instrument` is added to `Canvas::draw_*` methods (verdict §13 Step 9 explicitly declined).

**Cost.** Would add `Span` construction + thread-local lookup per draw call -- non-trivial for 1,000+ draws/frame.

**Verdict.** Already avoided. Document in ARCHITECTURE.md `## Friction log` that per-draw-call spans are NOT added by design.

---

## Outstanding refactors filed for U12 `ARCHITECTURE.md`

Each entry will land in `crates/flui-painting/ARCHITECTURE.md` `## Outstanding refactors` with named-blocker language:

1. **"Paint interning at construction"** -- requires `Paint: Hash + Eq` + per-canvas interning table + engine-side handle resolution + measured benchmark.
2. **"Flat-bytecode `Vec<u8>` `DisplayList` representation"** -- requires encoder + decoder + operation re-shape + measured benefit; very high blast radius.
3. **"`Path` Clone-on-Write (`Arc<[PathCommand]>`)"** -- requires `flui-types` breaking change.
4. **"Per-thread cosmic-text `FontSystem` (cosmic-text 0.13+)"** -- requires cosmic-text version bump.

---

## What was NOT changed in U9

Per the no-quick-wins memo's "concrete-blocker-with-named-dependency" exception, premature optimisation without measured benefit was deferred. The findings above all have real named blockers (external crate version bumps, breaking changes in dependency crates, new benchmark infrastructure). They are not "would touch X" deferrals.

U9 lands the documentation; U12 references it from the templated `ARCHITECTURE.md`. No source code changes in this step.

---

## Cross-references

- `docs/designs/2026-05-20-mythos-flui-painting-redesign.md` §9 (Data-Oriented Notes) and §12 (Rejected Designs entries "Paint interning at construction" and "Flat bytecode").
- `docs/plans/2026-05-20-004-feat-flui-painting-mythos-redesign-plan.md` U9 + U12.
- `crates/flui-painting/src/canvas/drawing.rs` -- file-level docstring already references this audit.
- `crates/flui-painting/src/display_list/command.rs` -- 29-variant enum where every variant pays the Matrix4 baking cost.
