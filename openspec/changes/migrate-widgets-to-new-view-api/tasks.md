# Tasks: Migrate Widgets to New View API

## Phase 1: Adapter Layer Foundation

### Task 1.1: Add RenderBoxExt extension methods
**Goal:** Provide backward-compatible builder methods for RenderObject → Element conversion

**Steps:**
1. Add `RenderBoxExt` trait to `flui_core/src/render.rs` with methods:
   - `fn leaf(self) -> Element` for Leaf arity
   - `fn child(self, child: impl IntoElement) -> Element` for Single arity
   - `fn children(self, children: Vec<Element>) -> Element` for Variable arity
   - `fn maybe_child(self, child: Option<Element>) -> Element` for Optional arity
2. Implement using `RenderView` wrapper from flui_rendering
3. Add unit tests for each method
4. Update flui_core/src/render.rs to export extension trait

**Validation:**
```bash
cargo test -p flui_core -- render_box_ext
cargo doc -p flui_core --no-deps --open
```

**Files Changed:**
- `crates/flui_core/src/render.rs`

---

### Task 1.2: Add Child and Children helper types
**Goal:** Ergonomic child widget storage with IntoElement conversion

**Steps:**
1. Verify `Child` and `Children` types exist in flui_view::children
2. Ensure they support `.into_element()` conversion
3. Add convenience methods: `Child::new()`, `Child::none()`, `Children::default()`

**Validation:**
```bash
cargo test -p flui-view -- children
```

**Files Changed:**
- `crates/flui-view/src/children.rs` (verify only)

---

## Phase 2: Basic Widgets (Priority 1)

### Task 2.1: Fix Text widget
**Goal:** Most commonly used widget - proof of concept for migration

**Steps:**
1. Update `Text::build()` to use `RenderParagraph::new(data).leaf()`
2. Ensure bon builder works: `Text::builder().data("Hello").size(16.0).build()`
3. Verify struct literal works: `Text { data: "Hello".into(), ..Default::default() }`
4. Add macro support: `text!("Hello")` and `text! { data: "Hello", size: 16.0 }`
5. Run existing tests

**Validation:**
```bash
cargo test -p flui_widgets -- text
cargo run --example text_widget
```

**Files Changed:**
- `crates/flui_widgets/src/basic/text.rs`

---

### Task 2.2: Fix Container widget
**Goal:** Most complex composition widget

**Steps:**
1. Update `Container::build()` to compose Padding, Align, DecoratedBox using adapter methods
2. Verify bon builder: `Container::builder().padding(...).color(...).child(...).build()`
3. Verify struct literal: `Container { padding: Some(...), color: Some(...), ..Default::default() }`
4. Add macro: `container! { padding: EdgeInsets::all(8.0), color: Color::BLUE }`
5. Fix all 11 convenience methods (colored, card, outlined, etc.)

**Validation:**
```bash
cargo test -p flui_widgets -- container
cargo run --example container_variants
```

**Files Changed:**
- `crates/flui_widgets/src/basic/container.rs`

---

### Task 2.3: Fix SizedBox, Padding, Center, Align
**Goal:** Core layout primitives

**Steps:**
1. Update each widget's `build()` to use `RenderXxx::new(...).child(child)` or `.maybe_child(child)`
2. Verify bon builder works for each
3. Verify struct literal works for each
4. Add macros where useful
5. Run tests for each

**Validation:**
```bash
cargo test -p flui_widgets -- basic
```

**Files Changed:**
- `crates/flui_widgets/src/basic/sized_box.rs`
- `crates/flui_widgets/src/basic/padding.rs`
- `crates/flui_widgets/src/basic/center.rs`
- `crates/flui_widgets/src/basic/align.rs`

---

### Task 2.4: Fix remaining basic widgets (16 widgets)
**Goal:** Complete basic module

**Widgets:**
- AspectRatio, Builder, Button, Card, ColoredBox, ConstrainedBox, CustomPaint
- DecoratedBox, Divider, Empty, FittedBox, LayoutBuilder, LimitedBox, SafeArea, VerticalDivider

**Steps:**
1. Update each widget's `build()` method
2. Ensure bon builder pattern works
3. Ensure struct literal pattern works
4. Add macros where useful
5. Run all basic widget tests

**Validation:**
```bash
cargo test -p flui_widgets -- basic
cargo build -p flui_widgets 2>&1 | grep "crates/flui_widgets/src/basic" | wc -l  # Should be 0
```

**Files Changed:**
- 16 files in `crates/flui_widgets/src/basic/`

---

## Phase 3: Layout Widgets (Priority 2)

### Task 3.1: Fix Row and Column
**Goal:** Most commonly used layout widgets

**Steps:**
1. Update `Row::build()` to use `RenderFlex::row().children(self.children)`
2. Update `Column::build()` similarly
3. Verify bon builder: `Row::builder().children(vec![...]).build()`
4. Verify struct literal: `Row { children: Children::from(vec![...]), ..Default::default() }`
5. Add macros: `row![child1, child2]` and `column![child1, child2]`
6. Test spacing methods: `Row::spaced(8.0, children)`

**Validation:**
```bash
cargo test -p flui_widgets -- row
cargo test -p flui_widgets -- column
cargo run --example row_column_layout
```

**Files Changed:**
- `crates/flui_widgets/src/layout/row.rs`
- `crates/flui_widgets/src/layout/column.rs`

---

### Task 3.2: Fix Stack and Positioned
**Goal:** Z-ordering layout

**Steps:**
1. Update `Stack::build()` to use `RenderStack::new().children(self.children)`
2. Update `Positioned::build()` to set PositionedMetadata
3. Verify bon builders work
4. Verify struct literals work
5. Add macros if useful

**Validation:**
```bash
cargo test -p flui_widgets -- stack
cargo test -p flui_widgets -- positioned
```

**Files Changed:**
- `crates/flui_widgets/src/layout/stack.rs`
- `crates/flui_widgets/src/layout/positioned.rs`

---

### Task 3.3: Fix Flex, Expanded, Flexible
**Goal:** Flex layout system

**Steps:**
1. Update `Flex::build()` for generic flex container
2. Update `Expanded::build()` to set FlexItemMetadata with flex=1
3. Update `Flexible::build()` to set FlexItemMetadata
4. Verify bon builders work
5. Verify struct literals work

**Validation:**
```bash
cargo test -p flui_widgets -- flex
```

**Files Changed:**
- `crates/flui_widgets/src/layout/flex.rs`
- `crates/flui_widgets/src/layout/expanded.rs`
- `crates/flui_widgets/src/layout/flexible.rs`

---

### Task 3.4: Fix remaining layout widgets (16 widgets)
**Goal:** Complete layout module

**Widgets:**
- Baseline, FractionallySizedBox, IndexedStack, IntrinsicHeight, IntrinsicWidth
- ListBody, OverflowBox, PositionedDirectional, RotatedBox, Scaffold
- ScrollController, SingleChildScrollView, SizedOverflowBox, Spacer, Wrap

**Steps:**
1. Update each widget's `build()` method
2. Ensure bon builder works
3. Ensure struct literal works
4. Add macros where useful
5. Run all layout tests

**Validation:**
```bash
cargo test -p flui_widgets -- layout
cargo build -p flui_widgets 2>&1 | grep "crates/flui_widgets/src/layout" | wc -l  # Should be 0
```

**Files Changed:**
- 16 files in `crates/flui_widgets/src/layout/`

---

## Phase 4: Interaction Widgets (Priority 3)

### Task 4.1: Fix GestureDetector
**Goal:** Primary interaction widget

**Steps:**
1. Update `GestureDetector::build()` to wrap child with pointer listener
2. Verify bon builder: `GestureDetector::builder().on_tap(|| {}).child(...).build()`
3. Verify struct literal works
4. Test all callbacks (on_tap, on_tap_down, on_tap_up, on_tap_cancel)

**Validation:**
```bash
cargo test -p flui_widgets -- gesture_detector
cargo run --example gesture_detector_demo
```

**Files Changed:**
- `crates/flui_widgets/src/gestures/detector.rs`

---

### Task 4.2: Fix MouseRegion, AbsorbPointer, IgnorePointer
**Goal:** Pointer interaction primitives

**Steps:**
1. Update each widget's `build()` method
2. Verify bon builders work
3. Verify struct literals work
4. Test pointer event handling

**Validation:**
```bash
cargo test -p flui_widgets -- interaction
```

**Files Changed:**
- `crates/flui_widgets/src/interaction/mouse_region.rs`
- `crates/flui_widgets/src/interaction/absorb_pointer.rs`
- `crates/flui_widgets/src/interaction/ignore_pointer.rs`

---

## Phase 5: Visual Effects (Priority 4)

### Task 5.1: Fix visual effect widgets (13 widgets)
**Goal:** Styling and effects

**Widgets:**
- Opacity, Transform, ClipRRect, ClipRect, ClipOval, ClipPath
- BackdropFilter, Material, Offstage, PhysicalModel, RepaintBoundary, Visibility

**Steps:**
1. Update each widget's `build()` method
2. Ensure bon builder works
3. Ensure struct literal works
4. Test each effect renders correctly

**Validation:**
```bash
cargo test -p flui_widgets -- visual_effects
cargo run --example visual_effects_showcase
```

**Files Changed:**
- 13 files in `crates/flui_widgets/src/visual_effects/`

---

## Phase 6: Examples and Documentation

### Task 6.1: Create widget gallery example
**Goal:** Demonstrate all widget categories

**Steps:**
1. Create `examples/widget_gallery.rs`
2. Include sections for:
   - Basic widgets (Text, Container, SizedBox, Padding)
   - Layout widgets (Row, Column, Stack)
   - Interaction widgets (GestureDetector, buttons)
   - Visual effects (Opacity, ClipRRect, Transform)
3. Add navigation between sections
4. Test that example runs and renders correctly

**Validation:**
```bash
cargo run --example widget_gallery
```

**Files Changed:**
- `examples/widget_gallery.rs` (new)
- `examples/README.md` (new)

---

### Task 6.2: Update widget documentation
**Goal:** Working code examples in documentation

**Steps:**
1. Update guide documentation with working examples
2. Update IMPLEMENTATION_GUIDE.md with adapter layer usage
3. Add "Quick Start" section to README.md
4. Verify all doc examples compile: `cargo test --doc -p flui_widgets`

**Validation:**
```bash
cargo test --doc -p flui_widgets
cargo doc -p flui_widgets --no-deps --open
```

**Files Changed:**
- `crates/flui_widgets/README.md`
- `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md`
- `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md`

---

## Phase 7: Testing and Validation

### Task 7.1: Run full test suite
**Goal:** Ensure all widgets work correctly

**Steps:**
1. Run all widget tests: `cargo test -p flui_widgets`
2. Fix any test failures
3. Add missing tests for untested widgets
4. Achieve >80% test coverage for widget modules

**Validation:**
```bash
cargo test -p flui_widgets
cargo test --workspace
```

---

### Task 7.2: Verify build without errors
**Goal:** Zero compilation errors

**Steps:**
1. Build widgets crate: `cargo build -p flui_widgets`
2. Build workspace: `cargo build --workspace`
3. Check clippy: `cargo clippy -p flui_widgets -- -D warnings`
4. Format code: `cargo fmt --all`

**Validation:**
```bash
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

---

## Summary

**Total Tasks:** 17 tasks across 7 phases

**Estimated Timeline:**
- Phase 1 (Adapter): 4 hours
- Phase 2 (Basic): 8 hours
- Phase 3 (Layout): 6 hours
- Phase 4 (Interaction): 3 hours
- Phase 5 (Effects): 4 hours
- Phase 6 (Examples): 3 hours
- Phase 7 (Testing): 2 hours

**Total Effort:** ~30 hours (~4 days)

**Key Milestones:**
1. ✅ Adapter layer complete (enables all other work)
2. ✅ Basic + Layout widgets working (covers 80% of use cases)
3. ✅ All widgets compiling (100% coverage)
4. ✅ Widget gallery example working (visual verification)
5. ✅ All tests passing (quality assurance)

**Dependencies:**
- Tasks within each phase can be parallelized
- Phase 2-5 all depend on Phase 1 completion
- Phase 6-7 depend on Phase 2-5 completion
