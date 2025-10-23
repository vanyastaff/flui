# Implementation Plan: flui_rendering & flui_painting

## Overview

This plan outlines the step-by-step implementation of `flui_rendering` and `flui_painting` crates using the new Generic Architecture approach.

**Goal**: Implement world-class rendering infrastructure with zero-cost abstractions, achieving 10-100x performance improvements through Element-level caching.

## Phase 1: flui_painting Foundation (Core Visual Primitives)

**Priority**: HIGH - Required by all RenderObjects

### 1.1 Core Structure (Day 1)

**Files to create:**
- `src/lib.rs` - Main exports
- `src/decoration/mod.rs` - Decoration system
- `src/decoration/box_decoration.rs` - BoxDecoration struct
- `src/border/mod.rs` - Border system
- `src/border/border.rs` - Border struct
- `src/border/border_radius.rs` - BorderRadius struct
- `src/gradient/mod.rs` - Gradient system
- `src/shadow/mod.rs` - Shadow system

**Core types to implement:**
```rust
// decoration/box_decoration.rs
pub struct BoxDecoration {
    pub color: Option<Color>,
    pub border: Option<Border>,
    pub border_radius: Option<BorderRadius>,
    pub gradient: Option<Gradient>,
    pub box_shadow: Vec<BoxShadow>,
}

impl BoxDecoration {
    pub fn paint(&self, painter: &egui::Painter, rect: Rect);
}
```

**Tests**: Unit tests for each component (paint logic, edge cases)

### 1.2 Border System (Day 2)

**Files:**
- `border/border.rs` - Border struct with sides
- `border/border_side.rs` - Individual border side
- `border/border_radius.rs` - Corner radius

**Implementation:**
```rust
pub struct Border {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

pub struct BorderRadius {
    pub top_left: Radius,
    pub top_right: Radius,
    pub bottom_left: Radius,
    pub bottom_right: Radius,
}
```

**Tests**: Border painting, radius calculations

### 1.3 Gradient System (Day 3)

**Files:**
- `gradient/linear.rs` - LinearGradient
- `gradient/radial.rs` - RadialGradient
- `gradient/sweep.rs` - SweepGradient

**Implementation:**
```rust
pub enum Gradient {
    Linear(LinearGradient),
    Radial(RadialGradient),
    Sweep(SweepGradient),
}

impl Gradient {
    pub fn paint(&self, painter: &egui::Painter, rect: Rect);
}
```

**Tests**: Gradient rendering, color stops

### 1.4 Shadow System (Day 4)

**Files:**
- `shadow/box_shadow.rs` - BoxShadow struct

**Implementation:**
```rust
pub struct BoxShadow {
    pub color: Color,
    pub offset: Offset,
    pub blur_radius: f32,
    pub spread_radius: f32,
}

impl BoxShadow {
    pub fn paint(&self, painter: &egui::Painter, rect: Rect);
}
```

**Tests**: Shadow rendering, blur effects

**Phase 1 Completion Criteria:**
- ✅ All painting primitives implemented
- ✅ Integration with egui::Painter working
- ✅ Comprehensive unit tests (>90% coverage)
- ✅ Documentation with examples

---

## Phase 2: flui_rendering Core Infrastructure

**Priority**: HIGH - Foundation for all RenderObjects

### 2.1 RenderState & Bitflags (Day 5)

**Files to create:**
- `src/lib.rs` - Main exports
- `src/core/mod.rs` - Core module
- `src/core/render_state.rs` - Shared state
- `src/core/render_flags.rs` - Bitflags

**Implementation:**
```rust
// core/render_flags.rs
bitflags! {
    pub struct RenderFlags: u8 {
        const NEEDS_LAYOUT = 1 << 0;
        const NEEDS_PAINT = 1 << 1;
        const NEEDS_COMPOSITING_BITS_UPDATE = 1 << 2;
        const IS_REPAINT_BOUNDARY = 1 << 3;
    }
}

// core/render_state.rs
pub struct RenderState {
    pub size: Option<Size>,
    pub constraints: Option<BoxConstraints>,
    pub flags: RenderFlags,
    pub parent_data: Option<Box<dyn Any>>,
}
```

**Tests**: Flag operations, state management

### 2.2 RenderBoxMixin Trait (Day 6)

**Files:**
- `src/core/render_box_mixin.rs` - Common trait

**Implementation:**
```rust
pub trait RenderBoxMixin {
    fn state(&self) -> &RenderState;
    fn state_mut(&mut self) -> &mut RenderState;

    fn size(&self) -> Option<Size> { self.state().size }
    fn constraints(&self) -> Option<BoxConstraints> { self.state().constraints }

    fn mark_needs_layout(&mut self) {
        self.state_mut().flags.insert(RenderFlags::NEEDS_LAYOUT);
    }

    fn mark_needs_paint(&mut self) {
        self.state_mut().flags.insert(RenderFlags::NEEDS_PAINT);
    }
}
```

**Tests**: Trait method behavior

### 2.3 Generic Base Types (Day 7-8)

**Files:**
- `src/core/leaf_render_box.rs` - LeafRenderBox<T>
- `src/core/single_render_box.rs` - SingleRenderBox<T>
- `src/core/container_render_box.rs` - ContainerRenderBox<T>

**Implementation:**
```rust
// core/leaf_render_box.rs
pub struct LeafRenderBox<T> {
    state: RenderState,
    data: T,
}

impl<T> RenderBoxMixin for LeafRenderBox<T> {
    fn state(&self) -> &RenderState { &self.state }
    fn state_mut(&mut self) -> &mut RenderState { &mut self.state }
}

// core/single_render_box.rs
pub struct SingleRenderBox<T> {
    state: RenderState,
    data: T,
    child: Option<BoxedRenderObject>,
}

impl<T> RenderBoxMixin for SingleRenderBox<T> {
    fn state(&self) -> &RenderState { &self.state }
    fn state_mut(&mut self) -> &mut RenderState { &mut self.state }
}

// core/container_render_box.rs
pub struct ContainerRenderBox<T> {
    state: RenderState,
    data: T,
    children: Vec<BoxedRenderObject>,
}

impl<T> RenderBoxMixin for ContainerRenderBox<T> {
    fn state(&self) -> &RenderState { &self.state }
    fn state_mut(&mut self) -> &mut RenderState { &mut self.state }
}
```

**Tests**: Generic type instantiation, child management

### 2.4 DynRenderObject Integration (Day 9)

**Files:**
- `src/core/render_object.rs` - DynRenderObject impl

**Implementation:**
```rust
impl<T: 'static> DynRenderObject for LeafRenderBox<T>
where
    T: RenderLeafLogic,
{
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.state.constraints = Some(constraints);
        let size = self.data.compute_size(constraints);
        self.state.size = Some(size);
        self.state.flags.remove(RenderFlags::NEEDS_LAYOUT);
        size
    }

    fn paint(&mut self, context: &mut PaintContext) {
        self.data.paint(context, self.state.size.unwrap());
        self.state.flags.remove(RenderFlags::NEEDS_PAINT);
    }
}
```

**Tests**: DynRenderObject trait implementation

**Phase 2 Completion Criteria:**
- ✅ All 3 generic types implemented
- ✅ RenderBoxMixin working across all types
- ✅ DynRenderObject integration complete
- ✅ Comprehensive tests for all core types

---

## Phase 3: Initial RenderObjects (Essential Set)

**Priority**: MEDIUM - Demonstrate architecture

### 3.1 Layout Objects (Day 10-12)

**Order of implementation:**

1. **RenderPadding** (Single-child, simplest)
   - File: `src/objects/layout/padding.rs`
   - Type: `SingleRenderBox<PaddingData>`
   - Tests: padding calculations, child layout

2. **RenderConstrainedBox** (Single-child, constraints)
   - File: `src/objects/layout/constrained_box.rs`
   - Type: `SingleRenderBox<ConstrainedBoxData>`
   - Tests: min/max constraints

3. **RenderFlex** (Container, complex)
   - File: `src/objects/layout/flex.rs`
   - Type: `ContainerRenderBox<FlexData>`
   - Tests: row/column layout, flex factors, spacing

4. **RenderStack** (Container, layering)
   - File: `src/objects/layout/stack.rs`
   - Type: `ContainerRenderBox<StackData>`
   - Tests: positioned/non-positioned children

**Implementation pattern:**
```rust
// objects/layout/padding.rs
pub struct PaddingData {
    pub padding: EdgeInsets,
}

impl RenderSingleLogic for PaddingData {
    fn compute_size(
        &mut self,
        constraints: BoxConstraints,
        child: Option<&mut BoxedRenderObject>,
    ) -> Size {
        if let Some(child) = child {
            let child_constraints = constraints.deflate(self.padding);
            let child_size = child.layout(child_constraints);
            Size::new(
                child_size.width + self.padding.horizontal(),
                child_size.height + self.padding.vertical(),
            )
        } else {
            constraints.constrain(Size::ZERO)
        }
    }
}

pub type RenderPadding = SingleRenderBox<PaddingData>;
```

### 3.2 Effect Objects (Day 13-14)

1. **RenderOpacity** (Single-child, simple effect)
   - File: `src/objects/effects/opacity.rs`
   - Type: `SingleRenderBox<OpacityData>`

2. **RenderClipRect** (Single-child, clipping)
   - File: `src/objects/effects/clip_rect.rs`
   - Type: `SingleRenderBox<ClipRectData>`

3. **RenderDecoratedBox** (Single-child, uses flui_painting)
   - File: `src/objects/effects/decorated_box.rs`
   - Type: `SingleRenderBox<DecoratedBoxData>`
   - **Key**: Uses `BoxDecoration::paint()` from flui_painting

**Implementation pattern:**
```rust
// objects/effects/decorated_box.rs
pub struct DecoratedBoxData {
    pub decoration: BoxDecoration, // from flui_painting
}

impl RenderSingleLogic for DecoratedBoxData {
    fn paint(&self, context: &mut PaintContext, size: Size) {
        // Use flui_painting
        self.decoration.paint(
            &context.painter,
            Rect::from_size(size),
        );
    }
}

pub type RenderDecoratedBox = SingleRenderBox<DecoratedBoxData>;
```

### 3.3 Interaction Objects (Day 15)

1. **RenderPointerListener** (Single-child, events)
   - File: `src/objects/interaction/pointer_listener.rs`
   - Type: `SingleRenderBox<PointerListenerData>`

2. **RenderMouseRegion** (Single-child, hover)
   - File: `src/objects/interaction/mouse_region.rs`
   - Type: `SingleRenderBox<MouseRegionData>`

**Phase 3 Completion Criteria:**
- ✅ 9 essential RenderObjects implemented
- ✅ All use generic architecture (LeafBox/SingleBox/ContainerBox)
- ✅ RenderDecoratedBox demonstrates flui_painting integration
- ✅ Each RenderObject has comprehensive tests
- ✅ ~20 lines of code per RenderObject (vs 200+ before)

---

## Phase 4: Complete Implementation (Remaining 72 Objects)

**Priority**: LOW-MEDIUM - Expand coverage

### 4.1 Implementation Strategy

**Batch by complexity:**
1. **Simple Single-child** (Day 16-18): AspectRatio, LimitedBox, FractionallySizedBox, etc. (~15 objects)
2. **Simple Containers** (Day 19-21): IndexedStack, Wrap, Flow, etc. (~10 objects)
3. **Complex Layout** (Day 22-25): Table, CustomMultiChildLayout, etc. (~8 objects)
4. **Text Rendering** (Day 26-28): RenderParagraph, RenderEditable, etc. (~6 objects)
5. **Sliver System** (Day 29-35): SliverList, SliverGrid, SliverAppBar, etc. (~20 objects)
6. **Advanced Effects** (Day 36-38): BackdropFilter, PhysicalModel, etc. (~8 objects)
7. **Specialized** (Day 39-42): Semantics, AnnotatedRegion, etc. (~5 objects)

**For each batch:**
- Create files in appropriate `src/objects/` subdirectory
- Implement using generic architecture
- Write tests (unit + integration)
- Update RENDER_OBJECTS_CATALOG.md status

### 4.2 Directory Structure

```
src/objects/
├── layout/
│   ├── mod.rs
│   ├── padding.rs          ✅ Phase 3
│   ├── constrained_box.rs  ✅ Phase 3
│   ├── flex.rs             ✅ Phase 3
│   ├── stack.rs            ✅ Phase 3
│   ├── aspect_ratio.rs     ⏳ Phase 4
│   ├── limited_box.rs      ⏳ Phase 4
│   └── ... (20+ more)
├── effects/
│   ├── mod.rs
│   ├── opacity.rs          ✅ Phase 3
│   ├── clip_rect.rs        ✅ Phase 3
│   ├── decorated_box.rs    ✅ Phase 3
│   └── ... (15+ more)
├── interaction/
│   ├── mod.rs
│   ├── pointer_listener.rs ✅ Phase 3
│   ├── mouse_region.rs     ✅ Phase 3
│   └── ... (5+ more)
├── text/
│   ├── mod.rs
│   ├── paragraph.rs        ⏳ Phase 4
│   └── ... (6+ more)
├── sliver/
│   ├── mod.rs
│   ├── list.rs             ⏳ Phase 4
│   └── ... (20+ more)
└── media/
    ├── mod.rs
    ├── image.rs            ⏳ Phase 4
    └── ... (3+ more)
```

**Phase 4 Completion Criteria:**
- ✅ All 81 RenderObjects implemented
- ✅ Full test coverage
- ✅ Documentation with examples
- ✅ Performance benchmarks showing 10-100x improvements

---

## Success Metrics

### Performance Targets
- ✅ Layout cache hit rate: >95%
- ✅ Layout time (cache hit): <20ns
- ✅ Layout time (cache miss): <200ns
- ✅ Memory per RenderObject: <100 bytes

### Code Quality Targets
- ✅ Lines per RenderObject: ~20 (vs 200+ before)
- ✅ Test coverage: >90%
- ✅ Zero unsafe code (except where absolutely necessary)
- ✅ All Clippy lints pass

### Documentation Targets
- ✅ Every public API documented
- ✅ Examples for all RenderObjects
- ✅ Architecture guides complete
- ✅ Migration guide from old architecture

---

## Dependencies & Blockers

### External Dependencies
- ✅ `flui_core` - Element system with LayoutCache (already implemented)
- ✅ `flui_types` - Basic types (Size, Offset, etc.)
- ⏳ `egui 0.33` - Rendering backend (migration needed)

### Internal Dependencies
```
flui_painting (Phase 1)
    ↓
flui_rendering core (Phase 2)
    ↓
flui_rendering objects (Phase 3-4)
```

---

## Timeline Summary

| Phase | Duration | Status |
|-------|----------|--------|
| Phase 1: flui_painting | 4 days | ⏳ Ready to start |
| Phase 2: Core Infrastructure | 5 days | ⏳ Waiting on Phase 1 |
| Phase 3: Essential Objects | 6 days | ⏳ Waiting on Phase 2 |
| Phase 4: Complete Coverage | 27 days | ⏳ Waiting on Phase 3 |
| **Total** | **~42 days** | **6 weeks** |

---

## Next Steps

1. **Immediate**: Start Phase 1.1 - Implement flui_painting core structure
2. **Day 1-4**: Complete flui_painting foundation
3. **Day 5-9**: Implement flui_rendering core infrastructure
4. **Day 10-15**: Implement essential RenderObjects
5. **Day 16-42**: Complete remaining 72 RenderObjects

**Let's begin with Phase 1.1!**
