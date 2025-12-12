## 1. Clean Code Foundation

- [ ] 1.1 Run `cargo clippy -p flui_rendering` and catalog all warnings
- [ ] 1.2 Remove unused imports in `src/object.rs`
- [ ] 1.3 Remove unused imports in `src/children/*.rs`
- [ ] 1.4 Remove unused imports in `src/mixins/*.rs`
- [ ] 1.5 Prefix intentionally unused variables with `_`
- [ ] 1.6 Remove dead code (`ProtocolIdentifier`, `LayoutProtocol` if unused)
- [ ] 1.7 Create `src/error.rs` with `RenderError` enum
- [ ] 1.8 Add error variants: `ConstraintViolation`, `ChildLayoutFailed`, `InvalidState`
- [ ] 1.9 Replace `unwrap()` in `pipeline_owner.rs` with proper error handling
- [ ] 1.10 Replace `unwrap()` in `painting_context.rs` with proper error handling
- [ ] 1.11 Audit visibility: mark internal helpers as `pub(crate)`
- [ ] 1.12 Verify `cargo clippy -p flui_rendering -- -D warnings` passes

## 2. Complete Pipeline Integration

### 2.1 Layout Pipeline
- [ ] 2.1.1 Implement depth-sorted dirty node processing in `flush_layout()`
- [ ] 2.1.2 Add relayout boundary detection logic
- [ ] 2.1.3 Wire `perform_layout(constraints)` calls for dirty nodes
- [ ] 2.1.4 Handle `parent_uses_size` flag for boundary optimization
- [ ] 2.1.5 Add `#[tracing::instrument]` to `flush_layout()`

### 2.2 Paint Pipeline
- [ ] 2.2.1 Implement reverse-depth dirty node processing in `flush_paint()`
- [ ] 2.2.2 Add repaint boundary detection and optimization
- [ ] 2.2.3 Handle detached layer graceful skipping
- [ ] 2.2.4 Add `#[tracing::instrument]` to `flush_paint()`

### 2.3 PaintingContext Completion
- [ ] 2.3.1 Implement `paint_child(child_id, offset)` with boundary handling
- [ ] 2.3.2 Implement `push_opacity(alpha, offset, painter_fn)`
- [ ] 2.3.3 Implement `push_clip_rect(rect, clip_behavior, painter_fn)`
- [ ] 2.3.4 Implement `push_transform(matrix, painter_fn)`
- [ ] 2.3.5 Implement `push_clip_rrect(rrect, clip_behavior, painter_fn)`

### 2.4 Compositing Bits
- [ ] 2.4.1 Implement `flush_compositing_bits()` depth-sorted update
- [ ] 2.4.2 Wire `needs_compositing` propagation to ancestors

## 3. Developer-Friendly Mixin API

### 3.1 ProxyBox Mixin
- [ ] 3.1.1 Verify `ProxyBox<T>` delegates layout correctly
- [ ] 3.1.2 Add `compute_min_intrinsic_width()` default delegation
- [ ] 3.1.3 Add `compute_max_intrinsic_width()` default delegation
- [ ] 3.1.4 Add `compute_min_intrinsic_height()` default delegation
- [ ] 3.1.5 Add `compute_max_intrinsic_height()` default delegation

### 3.2 ShiftedBox Mixin
- [ ] 3.2.1 Verify `ShiftedBox<T>` applies child offset in paint
- [ ] 3.2.2 Add `compute_child_offset()` method for subclasses
- [ ] 3.2.3 Implement baseline offset adjustment

### 3.3 ContainerBox Mixin
- [ ] 3.3.1 Verify `ContainerBox<T>` iterates children correctly
- [ ] 3.3.2 Add `default_paint(context, offset)` helper
- [ ] 3.3.3 Add `default_hit_test_children(result, position)` helper
- [ ] 3.3.4 Document ParentData usage patterns

### 3.4 Documentation
- [ ] 3.4.1 Create `MIXIN_GUIDE.md` with usage examples
- [ ] 3.4.2 Add inline rustdoc to all mixin traits

## 4. Ergonomic Constructors

### 4.1 RenderFlex Convenience
- [ ] 4.1.1 Add `RenderFlex::row()` constructor
- [ ] 4.1.2 Add `RenderFlex::column()` constructor
- [ ] 4.1.3 Add `RenderFlex::row_centered()` variant
- [ ] 4.1.4 Add `RenderFlex::column_centered()` variant

### 4.2 RenderPadding Convenience
- [ ] 4.2.1 Add `RenderPadding::all(value)` constructor
- [ ] 4.2.2 Add `RenderPadding::symmetric(h, v)` constructor
- [ ] 4.2.3 Add `RenderPadding::only(left, top, right, bottom)` constructor

### 4.3 RenderAlign Convenience
- [ ] 4.3.1 Add `RenderAlign::center()` constructor
- [ ] 4.3.2 Add `RenderAlign::top_left()`, `top_right()`, etc.

### 4.4 Builder Patterns
- [ ] 4.4.1 Create `RenderStackBuilder` with fluent API
- [ ] 4.4.2 Create `RenderFlexBuilder` with fluent API
- [ ] 4.4.3 Document builder usage in README

## 5. Benchmarks & Property Tests

### 5.1 Benchmark Setup
- [ ] 5.1.1 Add criterion dev-dependency to Cargo.toml
- [ ] 5.1.2 Create `benches/layout_benchmarks.rs`
- [ ] 5.1.3 Create `benches/paint_benchmarks.rs`

### 5.2 Layout Benchmarks
- [ ] 5.2.1 Implement `bench_layout_flex_100_children`
- [ ] 5.2.2 Implement `bench_layout_deep_tree_100_levels`
- [ ] 5.2.3 Implement `bench_layout_wide_tree_1000_siblings`

### 5.3 Performance Benchmarks
- [ ] 5.3.1 Implement `bench_dirty_flag_propagation`
- [ ] 5.3.2 Implement `bench_repaint_boundary_skip`
- [ ] 5.3.3 Document baseline numbers in BENCHMARKS.md

### 5.4 Property Tests
- [ ] 5.4.1 Add proptest dev-dependency to Cargo.toml
- [ ] 5.4.2 Create `tests/property_tests.rs`
- [ ] 5.4.3 Test: layout output satisfies constraints
- [ ] 5.4.4 Test: relayout boundaries prevent parent dirty
- [ ] 5.4.5 Test: paint order matches z-order

## 6. Final Polish

- [ ] 6.1 Update README.md with new convenience APIs
- [ ] 6.2 Update PRODUCTION_PLAN.md to mark completed phases
- [ ] 6.3 Run full test suite: `cargo test -p flui_rendering`
- [ ] 6.4 Run benchmarks and document results
- [ ] 6.5 Final clippy check with all features enabled
