# Tasks: Refactor Rendering Contexts to Flutter Model

## 1. Preparation

- [ ] 1.1 Create `PaintingContext` struct in new file `painting_context.rs`
- [ ] 1.2 Implement core `PaintingContext` methods: `canvas()`, `paint_child()`
- [ ] 1.3 Implement layer push methods: `push_clip_rect()`, `push_opacity()`, `push_transform()`
- [ ] 1.4 Add complexity hints: `set_is_complex_hint()`, `set_will_change_hint()`
- [ ] 1.5 Verify `BoxHitTestResult` in `flui_interaction` has all needed methods

## 2. Update RenderBox Trait

- [ ] 2.1 Rename `layout()` → `perform_layout()` with new signature
- [ ] 2.2 Update `paint()` signature to use `PaintingContext`
- [ ] 2.3 Update `hit_test()` signature to use `BoxHitTestResult` directly
- [ ] 2.4 Remove `LayoutContext` imports from `box_render.rs`
- [ ] 2.5 Update trait documentation with Flutter protocol references

## 3. Update RenderSliver Trait

- [ ] 3.1 Rename `layout()` → `perform_layout()` with new signature
- [ ] 3.2 Update `paint()` signature to use `PaintingContext`
- [ ] 3.3 Update `hit_test()` signature to use `SliverHitTestResult` directly
- [ ] 3.4 Remove `LayoutContext` imports from `sliver.rs`

## 4. Update Box Protocol Mixins

- [ ] 4.1 Update `ProxyBox<T>` to new signatures
- [ ] 4.2 Update `ShiftedBox<T>` to new signatures
- [ ] 4.3 Update `AligningShiftedBox<T>` to new signatures
- [ ] 4.4 Update `ContainerBox<T, PD>` to new signatures
- [ ] 4.5 Update `LeafBox<T>` to new signatures
- [ ] 4.6 Update mixin traits (`RenderProxyBoxMixin`, etc.)

## 5. Update Sliver Protocol Mixins

- [ ] 5.1 Update `ProxySliver<T>` to new signatures
- [ ] 5.2 Update `ShiftedSliver<T>` to new signatures
- [ ] 5.3 Update `ContainerSliver<T, PD>` to new signatures
- [ ] 5.4 Update `LeafSliver<T>` to new signatures

## 6. Update Tree Traits

- [ ] 6.1 Update `LayoutTree` trait — remove context-based methods
- [ ] 6.2 Update `PaintTree` trait — integrate with `PaintingContext`
- [ ] 6.3 Update `HitTestTree` trait — use result directly
- [ ] 6.4 Update `RenderTree` implementations

## 7. Cleanup Old Context Code

- [ ] 7.1 Remove `LayoutContext` struct and all type aliases
- [ ] 7.2 Remove `HitTestContext` struct and all type aliases
- [ ] 7.3 Rename/consolidate `PaintContext` → keep only `PaintingContext`
- [ ] 7.4 Update `context.rs` — remove deleted types, keep `PaintingContext`
- [ ] 7.5 Update `lib.rs` re-exports

## 8. Update Documentation

- [ ] 8.1 Update module docs in `lib.rs` with Flutter model explanation
- [ ] 8.2 Update `box_render.rs` documentation
- [ ] 8.3 Update mixins documentation  
- [ ] 8.4 Update or remove `impl/18_RENDERING_CONTEXTS.md`

## 9. Validation

- [ ] 9.1 Run `cargo build -p flui_rendering` — fix compilation errors
- [ ] 9.2 Run `cargo test -p flui_rendering` — fix test failures
- [ ] 9.3 Run `cargo clippy -p flui_rendering` — fix warnings
- [ ] 9.4 Update `mixin_demo.rs` example to new API
- [ ] 9.5 Verify downstream crates compile (`flui_widgets`, `flui_app`)

## Dependencies

- Tasks 2-5 depend on Task 1 (PaintingContext creation)
- Task 6 depends on Tasks 2-5 (trait updates)
- Task 7 depends on Task 6 (cleanup after updates)
- Task 8 can run in parallel with Task 7
- Task 9 must run after all other tasks
