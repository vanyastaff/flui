# Change: Polish flui_rendering for Production Readiness

## Why

The `flui_rendering` crate has an excellent architectural foundation that surpasses Flutter in key areas (compile-time arity validation, lock-free atomics, Ambassador+Deref mixins). To achieve production readiness and full Flutter API compatibility, several areas need polish:

1. **Code Quality**: 31 compiler warnings need cleanup
2. **Pipeline Gaps**: `flush_layout()` and `flush_paint()` not fully wired
3. **Error Handling**: `unwrap()` calls instead of proper `Result` types
4. **Developer Experience**: Missing convenience constructors and builders
5. **Validation**: No benchmarks proving performance claims

## What Changes

### Phase 1: Clean Code Foundation

**Goal**: Zero warnings, proper error types

- Remove unused imports across `object.rs`, `children/*.rs`, `mixins/*.rs`
- Create `RenderError` enum with descriptive variants:
  - `ConstraintViolation { expected, actual }`
  - `ChildLayoutFailed { child_id, source }`
  - `InvalidState { message }`
- Replace `unwrap()` with `?` operator in pipeline code
- Standardize visibility: `pub(crate)` for internals, `pub` for API

### Phase 2: Complete Pipeline Integration

**Goal**: Working layout/paint pipeline like Flutter's PipelineOwner

- `PipelineOwner::flush_layout()`:
  - Sort dirty nodes by depth (parents before children)
  - Call `perform_layout(constraints)` on each dirty relayout boundary
  - Respect `parent_uses_size` for relayout boundary detection
  - Add `tracing::instrument` spans for profiling

- `PipelineOwner::flush_paint()`:
  - Process dirty nodes deepest-first (children before parents)
  - Use `PaintingContext::repaint_composited_child()` for boundaries
  - Skip detached layers gracefully

- `PaintingContext` completion:
  - `paint_child(child, offset)` - handles repaint boundaries
  - `push_opacity(alpha, painter)` - creates OpacityLayer when compositing
  - `push_clip_rect(rect, clip_behavior, painter)` - ClipRectLayer
  - `push_transform(matrix, painter)` - TransformLayer

### Phase 3: Developer-Friendly Mixin API

**Goal**: Mixins that match Flutter naming and feel natural to use

| FLUI Mixin | Flutter Equivalent | Purpose |
|------------|-------------------|---------|
| `ProxyBox<T>` | `RenderProxyBox` | Pass-through single child |
| `ShiftedBox<T>` | `RenderShiftedBox` | Single child with offset |
| `AligningBox<T>` | `RenderAligningShiftedBox` | Alignment-based positioning |
| `ContainerBox<T>` | `ContainerRenderObjectMixin` | Multiple children |
| `LeafBox<T>` | (no equivalent) | Zero children, self-painting |

Add default implementations:
- `compute_min_intrinsic_width/height()` - delegate to child
- `compute_dry_layout()` - layout without side effects
- `default_paint()` / `default_hit_test_children()` for containers

### Phase 4: Ergonomic Constructors

**Goal**: Common patterns should be one-liners

```rust
// Instead of: RenderFlex::new(Axis::Horizontal, MainAxisAlignment::Start, ...)
let row = RenderFlex::row();
let column = RenderFlex::column();

// Instead of: RenderPadding::new(EdgeInsets::all(8.0))
let padded = RenderPadding::all(8.0);
let spaced = RenderPadding::symmetric(horizontal: 16.0, vertical: 8.0);

// Builder for complex configuration
let stack = RenderStack::builder()
    .alignment(Alignment::CENTER)
    .clip_behavior(ClipBehavior::HardEdge)
    .build();
```

### Phase 5: Benchmarks & Property Tests

**Goal**: Prove performance claims with data

Benchmarks (criterion):
- `layout_flex_100_children` - measure flex layout throughput
- `layout_deep_tree_100_levels` - measure constraint propagation
- `dirty_flag_propagation` - measure atomic flag performance
- `repaint_boundary_efficiency` - measure paint skipping

Property tests (proptest):
- Layout output always satisfies input constraints
- Relayout boundaries prevent unnecessary parent layouts
- Paint order matches child z-order (back to front)

## Impact

**Affected Code**:
- `crates/flui_rendering/src/pipeline_owner.rs`
- `crates/flui_rendering/src/painting_context.rs`
- `crates/flui_rendering/src/mixins/*.rs`
- `crates/flui_rendering/src/error.rs` (NEW)
- `crates/flui_rendering/benches/*.rs` (NEW)

**Breaking Changes**: None - all additive or internal

**Non-Goals** (handled by other proposals):
- RenderObject migration → `migrate-renderobjects-to-new-api`
- Tree abstraction refactor → `refactor-flui-tree-abstractions`

## Success Criteria

- [ ] `cargo clippy -p flui_rendering -- -D warnings` passes
- [ ] All pipeline phases have tracing instrumentation
- [ ] Benchmarks show ≥1.5x Flutter layout performance
- [ ] Property tests cover constraint satisfaction invariants
- [ ] README documents all convenience APIs
