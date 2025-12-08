# Generic Refactoring Checklist

Quick reference for generic architecture migration.

## 🎯 Goals

- ✅ Generic-first (no `dyn` in hot paths)
- ✅ Compile-time safety (catch errors early)
- ✅ Zero-cost abstractions (full monomorphization)
- ✅ Crate independence (minimal dependencies)
- ✅ Rust 1.90+ features

---

## 📋 Phase 1: flui-tree (START HERE!)

**Location:** `crates/flui-tree/`

### Arity System
- [ ] Add const generics to `Arity` trait
  - [ ] `const MIN: usize`
  - [ ] `const MAX: Option<usize>`
- [ ] Implement concrete arities:
  - [ ] `Leaf` (0 children)
  - [ ] `Single` (1 child)
  - [ ] `Optional` (0-1 children)
  - [ ] `Variable` (0+ children)
  - [ ] `Exact<const N: usize>` (exactly N)
  - [ ] `Range<const MIN: usize, const MAX: usize>`
- [ ] Add GAT for child accessors: `type Accessor<'a, T>`
- [ ] Test compile-time validation

**Verify:**
```bash
cargo build -p flui-tree
cargo test -p flui-tree
cargo clippy -p flui-tree -- -D warnings
```

---

## 📋 Phase 2: flui_rendering

**Location:** `crates/flui_rendering/`

### RenderElement Generic
- [x] Convert to `RenderElement<R, P>` ✓
- [ ] Add Arity: `RenderElement<R, P, A>`
- [ ] Use PhantomData for A
- [ ] Compile-time arity checks in `add_child()`
- [ ] Update all impl blocks

### Type Erasure Boundary
- [ ] Update `RenderElementNode` trait
  - [ ] Remove `render_id()` (direct ownership)
  - [ ] Add minimal interface only
  - [ ] Generic over P: `RenderElementNode<P>`
- [ ] Impl for `RenderElement<R, P, A>`
- [ ] Create `ElementNodeStorage` enum:
  ```rust
  enum ElementNodeStorage {
      Box { element: Box<dyn RenderElementNode<BoxProtocol>> },
      Sliver { element: Box<dyn RenderElementNode<SliverProtocol>> },
  }
  ```

### RenderTree Storage
- [x] Remove `render_objects` field ✓
- [ ] Make `RenderTree<T>` fully generic
- [ ] Update `RenderTreeStorage` trait
- [ ] Fix `perform_layout()` to work with generics
- [ ] No unsafe code (use context API instead)

### Context API
- [ ] Generic `LayoutContext<'a, R, P, A, T>`
- [ ] Generic `PaintContext<'a, R, P, A, T>`
- [ ] Generic `HitTestContext<'a, R, P, A, T>`
- [ ] Arity-aware methods (compile-time dispatch)

**Verify:**
```bash
cargo build -p flui_rendering
cargo test -p flui_rendering
# Check zero-cost
cargo asm flui_rendering::RenderElement::perform_layout --rust
```

---

## 📋 Phase 3: flui_core

**Location:** `crates/flui_core/`

### ElementTree
- [ ] Implement `RenderTreeStorage` for `ElementTree`
- [ ] Use `ElementNodeStorage` for heterogeneous elements
- [ ] Typed creation methods:
  - [ ] `create_box_element<R, A>(render_object: R) -> ElementId`
  - [ ] `create_sliver_element<R, A>(render_object: R) -> ElementId`
- [ ] Safe downcasting helpers

### TreeCoordinator
- [ ] Create in `flui_core` (not `flui_rendering`!)
- [ ] Coordinate 4 trees:
  - [ ] ViewTree
  - [ ] ElementTree
  - [ ] RenderTree<ElementTree>
  - [ ] LayerTree
- [ ] Full frame pipeline:
  - [ ] build_phase()
  - [ ] layout_phase()
  - [ ] paint_phase()
  - [ ] composite_phase()

**Verify:**
```bash
cargo build -p flui_core
cargo test -p flui_core
```

---

## 📋 Phase 4: Rust 1.90+ Features

### Apply Modern Features
- [ ] Const generics for arity
- [ ] GATs for child accessors
- [ ] Replace `OnceCell` with `LazyLock` (1.80+)
- [ ] Use `impl Trait` in trait returns (1.75+)
- [ ] Const functions where possible

**Verify:**
```bash
# Check MSRV
cargo +1.90 build --workspace
```

---

## 📋 Phase 5: Testing

### Unit Tests
- [ ] flui-tree arity validation
- [ ] flui_rendering generic elements
- [ ] flui_core tree coordination

### Integration Tests
- [ ] End-to-end render pipeline
- [ ] Protocol switching (Box ↔ Sliver)
- [ ] Arity enforcement

### Compile-Time Tests
- [ ] Use `#[compile_fail]` for arity violations
- [ ] Protocol mismatches caught

### Performance Tests
- [ ] Benchmark generic vs trait object
- [ ] Check assembly output
- [ ] Verify zero-cost abstractions

**Verify:**
```bash
cargo test --workspace --all-features
cargo bench --workspace
```

---

## 📋 Phase 6: Polish

### Code Quality
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings`
- [ ] Formatted: `cargo fmt --all -- --check`
- [ ] No unsafe without SAFETY comments
- [ ] Documentation for all public items

### Crate Independence
- [ ] Each crate builds standalone
- [ ] Minimal dependencies
- [ ] Check: `cargo tree -p <crate> --depth 1`

### Migration Path
- [ ] Remove old `core/` directory
- [ ] Remove old `view/` directory
- [ ] Update all imports to flat structure
- [ ] Update README.md

---

## ✅ Success Criteria

### Compile-Time
- [x] RenderElement<R, P> compiles
- [ ] RenderElement<R, P, A> compiles
- [ ] Arity violations caught at compile time
- [ ] Protocol mismatches caught at compile time

### Runtime
- [ ] All tests pass
- [ ] No panics in release mode
- [ ] Proper error handling

### Performance
- [ ] Zero-cost abstractions verified
- [ ] No regressions in benchmarks
- [ ] Binary size not increased

### Architecture
- [ ] Crates are independent
- [ ] Clear separation of concerns
- [ ] Type erasure only at boundary

---

## 🚀 Quick Start

```bash
# 1. Start with flui-tree
cd crates/flui-tree
# Implement const generics for Arity
cargo build && cargo test

# 2. Update flui_rendering
cd ../flui_rendering
# Add A parameter to RenderElement
cargo build && cargo test

# 3. Fix flui_core
cd ../flui_core
# Update ElementTree and TreeCoordinator
cargo build && cargo test

# 4. Test everything
cd ../..
cargo test --workspace --all-features
cargo clippy --workspace -- -D warnings
```

---

## 📚 Reference

- Full details: `docs/GENERIC_REFACTORING_ROADMAP.md`
- Current status: `docs/SUMMARY.md`
- Architecture: `docs/arch/CORE_ARCHITECTURE.md`
