# Design: Refactor Rendering Contexts to Flutter Model

## Context

Flutter's rendering architecture has evolved over years of production use. The key insight is that only the paint phase truly needs a context object because:
1. Canvas ownership must be managed (can change during child painting)
2. Layer composition requires state management
3. Paint recording needs to track bounds and complexity hints

Layout and hit-testing don't need wrapper contexts — they work with direct parameters.

## Goals

- Align with Flutter's proven `RenderObject` API design
- Simplify the crate by removing unnecessary abstractions
- Maintain type safety through Rust's type system (not through contexts)
- Keep arity validation at compile-time where useful

## Non-Goals

- Complete Flutter API parity (we keep Rust idioms where beneficial)
- Backwards compatibility (this is a breaking change)
- Performance optimization (architecture change, not performance-focused)

## Decisions

### Decision 1: Remove LayoutContext entirely

**What**: Delete `LayoutContext<A, P, T>` and all type aliases.

**Why**: Flutter stores constraints in the RenderObject and calls `performLayout()` with no arguments. The constraints are available via `this.constraints`. Children are laid out by calling `child.layout(constraints, parentUsesSize: true)` directly.

**Rust adaptation**:
```rust
// BEFORE (current)
fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;

// AFTER (Flutter model)
fn perform_layout(&mut self) -> Size;  // constraints accessed via self
// OR with explicit constraints:
fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
```

We choose explicit constraints parameter because Rust doesn't have implicit `this.constraints` like Dart.

### Decision 2: Rename PaintContext → PaintingContext and simplify

**What**: 
- Rename to match Flutter's `PaintingContext`
- Remove arity type parameter `A` (not needed for painting)
- Keep protocol parameter `P` for Box/Sliver distinction (optional)

**Why**: Paint doesn't need compile-time child count validation — it just iterates and paints children.

```rust
// BEFORE
pub struct PaintContext<'a, A: Arity, P: Protocol, T: PaintTree>

// AFTER  
pub struct PaintingContext<'a> {
    canvas: &'a mut Canvas,
    tree: &'a dyn PaintTree,
    // layer management...
}
```

**Key methods (Flutter API)**:
```rust
impl PaintingContext<'_> {
    fn canvas(&mut self) -> &mut Canvas;
    fn paint_child(&mut self, child: RenderId, offset: Offset);
    
    // Layer composition
    fn push_clip_rect(&mut self, rect: Rect, painter: impl FnOnce(&mut Self));
    fn push_opacity(&mut self, alpha: f32, painter: impl FnOnce(&mut Self));
    fn push_transform(&mut self, transform: Matrix4, painter: impl FnOnce(&mut Self));
    
    // Hints
    fn set_is_complex_hint(&mut self);
    fn set_will_change_hint(&mut self);
}
```

### Decision 3: Remove HitTestContext, use HitTestResult directly

**What**: Delete `HitTestContext<A, P, T>`, use `BoxHitTestResult`/`SliverHitTestResult`.

**Why**: Flutter's `hitTest()` takes a `BoxHitTestResult` (or `SliverHitTestResult`) and position. The result accumulates hit entries with transform tracking. No context wrapper needed.

```rust
// BEFORE
fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool;

// AFTER (Flutter model)
fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool;
```

`BoxHitTestResult` already exists in `flui_interaction` with:
- `add()` — add hit entry
- `push_transform()` / `pop_transform()` — transform stack
- `add_with_paint_offset()` — convenience for offset-based children

### Decision 4: Keep arity for RenderBox trait (layout only)

**What**: Keep `RenderBox<A: Arity>` for type-safe child access during layout.

**Why**: Layout benefits from compile-time child count validation:
- `RenderBox<Leaf>` — guaranteed 0 children
- `RenderBox<Single>` — guaranteed 1 child accessor
- `RenderBox<Variable>` — iterator over children

This prevents runtime errors like "expected 1 child, got 2".

```rust
pub trait RenderBox<A: Arity>: RenderObject {
    /// Layout with constraints (arity used for child access)
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
    
    /// Paint (no arity needed - just iterate children)
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    
    /// Hit test (no arity needed - test children in reverse z-order)
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool;
}
```

## Alternatives Considered

### Alternative A: Keep all three contexts
**Rejected**: Over-engineering. Flutter proved layout/hit-test don't need contexts.

### Alternative B: Remove arity entirely
**Rejected**: Arity provides valuable compile-time guarantees for layout. 

### Alternative C: Use visitor pattern for paint
**Rejected**: Flutter's `PaintingContext` pattern is simpler and well-understood.

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking all existing render objects | High | Provide migration guide, update all mixins |
| Learning curve for existing code | Medium | Document Flutter parallels clearly |
| Arity still needed for layout | Low | Keep arity for `RenderBox<A>` trait only |

## Migration Plan

### Step 1: Add new APIs alongside old
- Create `PaintingContext` (new name)
- Add `perform_layout()` method with new signature
- Keep old APIs temporarily

### Step 2: Migrate mixins
- Update `ProxyBox`, `ShiftedBox`, `ContainerBox`, `LeafBox`
- Update `ProxySliver`, `ShiftedSliver`, `ContainerSliver`, `LeafSliver`

### Step 3: Remove old APIs
- Delete `LayoutContext`
- Delete `HitTestContext`  
- Rename/consolidate `PaintContext` → `PaintingContext`
- Update re-exports in `lib.rs`

### Rollback
- Git revert if issues found
- Old APIs preserved in git history

## Open Questions

1. **Should we keep Protocol parameter for PaintingContext?**
   - Box and Sliver have different geometry types
   - Could use `dyn Any` for geometry or separate `BoxPaintingContext`/`SliverPaintingContext`
   
2. **How to handle child layout in perform_layout?**
   - Need access to tree to call `child.layout()`
   - Options: pass tree reference, use callback, store tree in RenderObject

3. **Should hit_test take tree reference?**
   - Needed to call `child.hit_test()`
   - Flutter uses parent data for child access
