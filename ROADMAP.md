# Flui Framework - Development Roadmap (Updated)

> Flutter-inspired declarative UI framework built on egui 0.33
> **Focus: Low-Level & Middle-Level Infrastructure**

## 📋 Table of Contents

- [Project Status](#project-status)
- [Architecture Overview](#architecture-overview)
- [Completed Work](#completed-work)
- [Low-Level Roadmap](#low-level-roadmap)
- [Middle-Level Roadmap](#middle-level-roadmap)
- [Dependencies](#dependencies)
- [Success Metrics](#success-metrics)

---

## 🎯 Project Status

**Current Phase:** Core Infrastructure Complete ✅
**Next Focus:** Widget Implementations & Layout Algorithms

### What's Done
- ✅ Complete type system (flui_types - 524 tests)
- ✅ Foundation utilities (keys, change notifications, listenable)
- ✅ Full Widget/Element/RenderObject architecture (flui_core - 49 tests)
- ✅ RenderObject trait with downcast-rs integration
- ✅ RenderObjectElement with lifecycle management
- ✅ Basic rendering infrastructure (RenderBox, RenderProxyBox)

### What's Next
- 🎯 **IMMEDIATE:** Concrete widget implementations (Container, Row, Column)
- 🎯 **IMMEDIATE:** Layout algorithms (RenderFlex, RenderStack, RenderPadding)
- ⏳ Painting system (decorations, borders, gradients)
- ⏳ Platform integration (FluiApp, event loop)

---

## 🏗 Architecture Overview

### Three-Tree Architecture

```
Widget Tree (Immutable)
    ↓ create_element()
Element Tree (Mutable State)
    ↓ attach_render_object()
RenderObject Tree (Layout & Paint)
```

### Crate Structure

```
flui/
├── flui_types/          ✅ COMPLETE (524 тести) - Geometry, Layout, Styling, Typography...
├── flui_foundation/     ✅ COMPLETE - Keys, ChangeNotifier, Listenable, Platform
├── flui_core/           ✅ COMPLETE (49 тестів) - Widget/Element/RenderObject система
│   ├── Widget система   ✅ DynClone + Downcast
│   ├── Element система  ✅ DowncastSync + RenderObjectElement
│   ├── RenderObject     ✅ DowncastSync (перенесено з flui_rendering)
│   └── ParentData       ✅ DowncastSync
├── flui_rendering/      ✅ BASIC (15 тестів) - RenderBox, RenderProxyBox
│   ├── RenderBox        ✅ Базова box protocol реалізація
│   ├── RenderProxyBox   ✅ Passes layout to child
│   ├── RenderFlex       🎯 NEXT - Row/Column layout algorithm
│   ├── RenderStack      ⏳ TODO - Positioned layout
│   └── RenderPadding    ⏳ TODO - Padding layout
├── flui_widgets/        🎯 NEXT - Widget implementations
├── flui_painting/       ⏳ TODO - Decorations, borders, gradients
├── flui_animation/      ⏳ TODO - AnimationController, Tweens
├── flui_gestures/       ⏳ TODO - GestureDetector, recognizers
└── flui_scheduler/      ⏳ TODO - Frame scheduling, Ticker
```

---

## ✅ Completed Work

### 1. flui_types (100%)

**Geometry Module:**
- `Point`, `Size`, `Offset`, `Rect` - all with full operations
- Comprehensive conversion implementations
- 40+ unit tests

**Layout Module:**
- `Axis`, `AxisDirection`, `Orientation`, `VerticalDirection`
- `Alignment` with 9 standard constants (topLeft, center, etc.)
- `MainAxisAlignment` (Start, End, Center, SpaceBetween, SpaceAround, SpaceEvenly)
- `CrossAxisAlignment` (Start, End, Center, Stretch, Baseline)
- `MainAxisSize` (Min/Max)

**Key Files:**
- [geometry/point.rs](crates/flui_types/src/geometry/point.rs)
- [geometry/size.rs](crates/flui_types/src/geometry/size.rs)
- [geometry/offset.rs](crates/flui_types/src/geometry/offset.rs)
- [geometry/rect.rs](crates/flui_types/src/geometry/rect.rs)
- [layout/alignment.rs](crates/flui_types/src/layout/alignment.rs)
- [layout/axis.rs](crates/flui_types/src/layout/axis.rs)

---

### 2. flui_foundation (85%)

**Key System (COMPLETE):**
```rust
pub trait Key: Any + Debug + Send + Sync
pub struct UniqueKey           // Unique per instance (atomic counter)
pub struct ValueKey<T>         // Based on value hash
pub struct StringKey           // String-based key
pub struct IntKey              // Integer-based key
pub enum WidgetKey             // Optional key wrapper
```

**Change Notification (COMPLETE):**
```rust
pub trait Listenable
pub struct ChangeNotifier      // Observable pattern with listeners
pub struct ValueNotifier<T>    // Holds value, notifies on changes
pub struct MergedListenable    // Combine multiple listenables
```

**Platform Types (PARTIAL):**
```rust
pub enum TargetPlatform        // Windows, Linux, MacOS, Web, Android, iOS
pub enum PlatformBrightness    // Light, Dark
```

**Key Files:**
- [key.rs](crates/flui_foundation/src/key.rs) - 328 lines
- [change_notifier.rs](crates/flui_foundation/src/change_notifier.rs) - 323 lines
- [platform.rs](crates/flui_foundation/src/platform.rs)

**Missing:**
- Full diagnostics system
- ObserverList (alternative to Arc<Mutex<Vec<_>>>)

---

### 3. flui_core (100%) ✅

**Widget System (COMPLETE with downcast-rs):**
```rust
pub trait Widget: DynClone + Downcast + Debug + Send + Sync {
    fn create_element(&self) -> Box<dyn Element>;
    fn key(&self) -> Option<&dyn Key>;
    fn can_update(&self, other: &dyn Widget) -> bool;
}

pub trait StatelessWidget: Debug + Clone + Send + Sync + 'static {
    fn build(&self, context: &BuildContext) -> Box<dyn Widget>;
}

pub trait StatefulWidget: Debug + Clone + Send + Sync + 'static {
    type State: State;
    fn create_state(&self) -> Self::State;
}

pub trait State: DowncastSync + Debug {
    fn build(&mut self, context: &BuildContext) -> Box<dyn Widget>;
    fn init_state(&mut self) {}
    fn did_update_widget(&mut self, old_widget: &dyn Any) {}
    fn dispose(&mut self) {}
}

pub trait InheritedWidget: Debug + Clone + Send + Sync + 'static {
    type Data;
    fn data(&self) -> &Self::Data;
    fn child(&self) -> &dyn Widget;
    fn update_should_notify(&self, old: &Self) -> bool;
}

pub trait RenderObjectWidget: Widget {
    fn create_render_object(&self) -> Box<dyn RenderObject>;
    fn update_render_object(&self, render_object: &mut dyn RenderObject);
}
```

**Element System (COMPLETE with downcast-rs):**
```rust
pub struct ElementId(u64);     // Unique identifier (atomic counter)

pub trait Element: DowncastSync + Debug {
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);
    fn unmount(&mut self);
    fn update(&mut self, new_widget: Box<dyn Any>);
    fn rebuild(&mut self);
    fn mark_dirty(&mut self);
    fn id(&self) -> ElementId;
    fn parent(&self) -> Option<ElementId>;
    fn key(&self) -> Option<&dyn Key>;
}

pub struct ComponentElement<W: StatelessWidget>  // ✅
pub struct StatefulElement                        // ✅
pub struct InheritedElement<W: InheritedWidget>  // ✅
pub struct RenderObjectElement<W: RenderObjectWidget> // ✅ NEW!
```

**RenderObject System (COMPLETE):**
```rust
pub trait RenderObject: DowncastSync + Debug {
    fn layout(&mut self, constraints: BoxConstraints) -> Size;
    fn paint(&self, painter: &egui::Painter, offset: Offset);
    fn size(&self) -> Size;
    fn needs_layout(&self) -> bool;
    fn mark_needs_layout(&mut self);
    fn needs_paint(&self) -> bool;
    fn mark_needs_paint(&mut self);
    fn hit_test(&self, position: Offset) -> bool;
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject));
}
```

**ParentData System (COMPLETE):**
```rust
pub trait ParentData: DowncastSync + Debug {}

pub struct ContainerParentData<ChildId>    // ✅
pub struct BoxParentData                   // ✅
pub struct ContainerBoxParentData<ChildId> // ✅
```

**Key Files:**
- [widget.rs](crates/flui_core/src/widget.rs) - Widget, StatelessWidget, StatefulWidget, State
- [element.rs](crates/flui_core/src/element.rs) - Element trait, ComponentElement, StatefulElement, RenderObjectElement
- [render_object.rs](crates/flui_core/src/render_object.rs) - RenderObject trait (moved from flui_rendering)
- [render_object_widget.rs](crates/flui_core/src/render_object_widget.rs) - RenderObjectWidget traits
- [inherited_widget.rs](crates/flui_core/src/inherited_widget.rs) - InheritedWidget, InheritedElement
- [parent_data.rs](crates/flui_core/src/parent_data.rs) - ParentData система
- [constraints.rs](crates/flui_core/src/constraints.rs) - BoxConstraints
- [build_context.rs](crates/flui_core/src/build_context.rs) - BuildContext

**Test Coverage:** 49 tests ✅

---

### 4. flui_rendering (30%) - Basic Infrastructure

**RenderObject Trait (MOVED TO flui_core):**
```rust
// RenderObject trait теперь находится в flui_core
// flui_rendering re-export его для удобства
pub use flui_core::RenderObject;
```

**RenderBox (COMPLETE):**
```rust
pub struct RenderBox {
    size: Size,
    constraints: Option<BoxConstraints>,
    needs_layout: bool,
    needs_paint: bool,
}

impl RenderObject for RenderBox { /* ... */ }
```

**RenderProxyBox (COMPLETE):**
```rust
pub struct RenderProxyBox {
    base: RenderBox,
    child: Option<Box<dyn RenderObject>>,
}

impl RenderObject for RenderProxyBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Passes constraints to child
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // Visits single child
    }
}
```

**Key Files:**
- [render_object.rs](crates/flui_rendering/src/render_object.rs) - Re-export from flui_core
- [render_box.rs](crates/flui_rendering/src/render_box.rs) - Basic implementations

**Test Coverage:** 99 tests ✅ (+84 новых!)

**Implemented RenderObjects:**
- ✅ **RenderFlex** (550 строк, 15 тестів) - Row/Column layout algorithm
- ✅ **RenderPadding** (280 строк, 8 тестів) - Padding layout
- ✅ **RenderStack** (330 строк, 13 тестів) - Positioned layout with StackFit
- ✅ **RenderConstrainedBox** (180 строк, 10 тестів) - SizedBox/ConstrainedBox
- ✅ **RenderDecoratedBox** (320 строк, 10 тестів) - BoxDecoration painting (2025-01-18)
- ✅ **RenderAspectRatio** (390 строк, 17 тестів) - Aspect ratio support (2025-01-18)

**Missing (Priority MEDIUM):**
- ⏳ **RenderLimitedBox** - Ограничивает размер при unbounded constraints
- ⏳ **RenderIndexedStack** - Stack с visible index
- ⏳ **RenderWrap** - Wrap layout
- ⏳ Layer tree for compositing
- ⏳ Paint caching

---

## 🔧 Low-Level Roadmap

### Phase 1: Painting System (Week 1-2)

**Priority: HIGH**

Create the painting infrastructure for drawing decorations, borders, and backgrounds.

#### 1.1 EdgeInsets & Spacing

```rust
// crates/flui_painting/src/edge_insets.rs
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub const ZERO: Self;
    pub fn all(value: f32) -> Self;
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self;
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self;
    pub fn horizontal(&self) -> f32;
    pub fn vertical(&self) -> f32;
    pub fn inflate_rect(&self, rect: Rect) -> Rect;
    pub fn deflate_rect(&self, rect: Rect) -> Rect;
}
```

**Files to create:**
- `crates/flui_painting/src/edge_insets.rs`
- `crates/flui_painting/src/edge_insets_geometry.rs`

**Tests:**
- EdgeInsets calculations
- Rect inflation/deflation
- Directional insets resolution

**Time:** 2 days

---

#### 1.2 Borders & Border Radius

```rust
// crates/flui_painting/src/borders.rs
pub struct BorderSide {
    pub color: egui::Color32,
    pub width: f32,
    pub style: BorderStyle,
}

pub enum BorderStyle {
    None,
    Solid,
    // Future: Dashed, Dotted
}

pub struct Border {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

impl Border {
    pub fn all(side: BorderSide) -> Self;
    pub fn symmetric(vertical: BorderSide, horizontal: BorderSide) -> Self;
    pub fn paint(&self, painter: &egui::Painter, rect: Rect);
}

// crates/flui_painting/src/border_radius.rs
pub struct BorderRadius {
    pub top_left: Radius,
    pub top_right: Radius,
    pub bottom_right: Radius,
    pub bottom_left: Radius,
}

impl BorderRadius {
    pub fn all(radius: Radius) -> Self;
    pub fn circular(radius: f32) -> Self;
    pub fn to_rrect(&self, rect: Rect) -> egui::Rounding;
}

pub struct Radius {
    pub x: f32,
    pub y: f32,
}

impl Radius {
    pub fn circular(radius: f32) -> Self;
    pub fn elliptical(x: f32, y: f32) -> Self;
}
```

**Files to create:**
- `crates/flui_painting/src/borders.rs`
- `crates/flui_painting/src/border_radius.rs`

**Tests:**
- Border painting on egui canvas
- BorderRadius conversions
- Border side combinations

**Time:** 3 days

---

#### 1.3 Box Decoration

```rust
// crates/flui_painting/src/decoration.rs
pub trait Decoration: Debug {
    fn paint(&self, painter: &egui::Painter, rect: Rect);
}

pub struct BoxDecoration {
    pub color: Option<egui::Color32>,
    pub border: Option<Border>,
    pub border_radius: Option<BorderRadius>,
    pub box_shadow: Vec<BoxShadow>,
    // Future: gradient, image
}

impl BoxDecoration {
    pub fn new() -> Self;
    pub fn with_color(mut self, color: egui::Color32) -> Self;
    pub fn with_border(mut self, border: Border) -> Self;
    pub fn with_border_radius(mut self, radius: BorderRadius) -> Self;
}

pub struct BoxShadow {
    pub color: egui::Color32,
    pub offset: Offset,
    pub blur_radius: f32,
    pub spread_radius: f32,
}
```

**Files to create:**
- `crates/flui_painting/src/decoration.rs`
- `crates/flui_painting/src/box_decoration.rs`
- `crates/flui_painting/src/box_shadow.rs`

**Tests:**
- BoxDecoration painting
- Shadow rendering
- Combined decoration effects

**Time:** 3 days

---

#### 1.4 Text Styles (Basic)

```rust
// crates/flui_painting/src/text_style.rs
pub struct TextStyle {
    pub color: Option<egui::Color32>,
    pub font_size: Option<f32>,
    pub font_family: Option<String>,
    pub font_weight: Option<FontWeight>,
    // Future: letter_spacing, word_spacing, height, decoration
}

pub enum FontWeight {
    Thin,       // 100
    ExtraLight, // 200
    Light,      // 300
    Normal,     // 400
    Medium,     // 500
    SemiBold,   // 600
    Bold,       // 700
    ExtraBold,  // 800
    Black,      // 900
}
```

**Files to create:**
- `crates/flui_painting/src/text_style.rs`

**Tests:**
- TextStyle merging
- Font weight conversions
- egui integration

**Time:** 2 days

**Total Phase 1 Time:** ~10 days

---

### Phase 2: Layout System (Week 3-4)

**Priority: CRITICAL**

Implement the core layout algorithms that power the framework.

#### 2.1 RenderFlex (Row/Column)

```rust
// crates/flui_rendering/src/flex.rs
pub struct RenderFlex {
    base: RenderBox,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    children: Vec<RenderFlexChild>,
}

struct RenderFlexChild {
    child: Box<dyn RenderObject>,
    flex: Option<i32>,  // None for non-flexible children
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self;
    pub fn add_child(&mut self, child: Box<dyn RenderObject>, flex: Option<i32>);

    // Layout algorithm (Flutter-compatible)
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
}

impl RenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // 1. Measure inflexible children
        // 2. Distribute remaining space to flexible children
        // 3. Position children along main axis
        // 4. Align children on cross axis
        // 5. Determine final size
    }
}
```

**Algorithm Steps:**
1. Layout non-flexible children with loose constraints
2. Calculate remaining space
3. Distribute space to flexible children based on flex factor
4. Position children along main axis (respect alignment)
5. Align children on cross axis (respect cross alignment)

**Files to create:**
- `crates/flui_rendering/src/flex.rs` - Core algorithm (~400 lines)
- `crates/flui_rendering/src/flex_parent_data.rs` - Parent data

**Tests:**
- Flex layout with various alignments
- Flexible/Expanded children
- Overflow handling
- Nested flex layouts

**Time:** 5 days

---

#### 2.2 RenderStack (Positioned)

```rust
// crates/flui_rendering/src/stack.rs
pub struct RenderStack {
    base: RenderBox,
    alignment: Alignment,
    fit: StackFit,
    children: Vec<StackChild>,
}

struct StackChild {
    child: Box<dyn RenderObject>,
    positioned_data: Option<PositionedData>,
}

pub struct PositionedData {
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

pub enum StackFit {
    Loose,      // Children size themselves
    Expand,     // Children expand to stack size
    PassThrough, // Stack sizes to constraints
}

impl RenderObject for RenderStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // 1. Layout non-positioned children
        // 2. Determine stack size
        // 3. Layout positioned children with calculated constraints
        // 4. Position all children
    }
}
```

**Files to create:**
- `crates/flui_rendering/src/stack.rs`
- `crates/flui_rendering/src/positioned.rs`

**Tests:**
- Stack with non-positioned children
- Positioned children (left, top, right, bottom)
- Combined width/height with positioning
- Stack alignment

**Time:** 3 days

---

#### 2.3 RenderPadding

```rust
// crates/flui_rendering/src/padding.rs
pub struct RenderPadding {
    base: RenderProxyBox,
    padding: EdgeInsets,
}

impl RenderObject for RenderPadding {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner_constraints = constraints.deflate(self.padding);
        let child_size = self.child.layout(inner_constraints);
        Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        )
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        let child_offset = offset + Offset::new(self.padding.left, self.padding.top);
        self.child.paint(painter, child_offset);
    }
}
```

**Files to create:**
- `crates/flui_rendering/src/padding.rs`

**Tests:**
- Padding layout calculations
- Child positioning
- Constraint propagation

**Time:** 2 days

**Total Phase 2 Time:** ~10 days

---

### Phase 3: Element Tree Management (Week 5)

**Priority: CRITICAL**

Implement the element tree that manages widget lifecycle and rebuilds.

#### 3.1 ElementTree

```rust
// crates/flui_core/src/element_tree.rs
pub struct ElementTree {
    root: Option<ElementId>,
    elements: HashMap<ElementId, Box<dyn Element>>,
    dirty_elements: Vec<ElementId>,
    next_frame_callbacks: Vec<Box<dyn FnOnce()>>,
}

impl ElementTree {
    pub fn new() -> Self;

    pub fn mount_root(&mut self, widget: Box<dyn Widget>) -> ElementId;
    pub fn unmount_root(&mut self);

    pub fn mark_dirty(&mut self, id: ElementId);
    pub fn rebuild_dirty(&mut self);

    pub fn schedule_frame_callback(&mut self, callback: impl FnOnce() + 'static);
}
```

**Files to create:**
- `crates/flui_core/src/element_tree.rs`
- `crates/flui_core/src/build_owner.rs` - Manages build process

**Tests:**
- Element tree construction
- Dirty marking and rebuilds
- Element lifecycle (mount/update/unmount)

**Time:** 4 days

---

#### 3.2 StatefulElement Implementation

```rust
// Complete the StatefulElement implementation
pub struct StatefulElement {
    id: ElementId,
    widget: Box<dyn StatefulWidget>,
    state: Box<dyn State>,  // Add this!
    child: Option<ElementId>,
    dirty: bool,
}

impl StatefulElement {
    pub fn set_state<F: FnOnce(&mut dyn State)>(&mut self, callback: F) {
        callback(&mut *self.state);
        self.mark_dirty();
    }
}
```

**Files to update:**
- [element.rs](crates/flui_core/src/element.rs)

**Tests:**
- StatefulWidget state preservation
- setState triggering rebuilds
- State lifecycle callbacks

**Time:** 2 days

---

#### 3.3 InheritedWidget

```rust
// crates/flui_core/src/inherited_widget.rs
pub trait InheritedWidget: Widget {
    fn update_should_notify(&self, old: &Self) -> bool;
    fn data(&self) -> &dyn Any;
}

pub struct InheritedElement {
    id: ElementId,
    widget: Box<dyn InheritedWidget>,
    child: Option<ElementId>,
    dependents: HashSet<ElementId>,  // Elements that depend on this
}

impl InheritedElement {
    pub fn notify_dependents(&mut self, tree: &mut ElementTree) {
        for dependent_id in &self.dependents {
            tree.mark_dirty(*dependent_id);
        }
    }
}

// BuildContext extension
impl BuildContext {
    pub fn depend_on_inherited<T: InheritedWidget>(&self) -> Option<&T>;
}
```

**Files to create:**
- `crates/flui_core/src/inherited_widget.rs`

**Tests:**
- InheritedWidget data access
- Dependent tracking
- Rebuild notifications

**Time:** 3 days

**Total Phase 3 Time:** ~9 days

---

### Phase 4: Basic Widgets (Week 6-7)

**Priority: HIGH**

Implement the essential widget set.

#### 4.1 Container Widget

```rust
// crates/flui_widgets/src/basic/container.rs
#[derive(Debug, Clone)]
pub struct Container {
    key: Option<WidgetKey>,
    width: Option<f32>,
    height: Option<f32>,
    padding: Option<EdgeInsets>,
    margin: Option<EdgeInsets>,
    color: Option<egui::Color32>,
    decoration: Option<BoxDecoration>,
    alignment: Option<Alignment>,
    child: Option<Box<dyn Widget>>,
}

impl Container {
    pub fn new() -> Self;
    pub fn with_child(mut self, child: Box<dyn Widget>) -> Self;
    pub fn with_color(mut self, color: egui::Color32) -> Self;
    pub fn with_padding(mut self, padding: EdgeInsets) -> Self;
    pub fn with_size(mut self, width: f32, height: f32) -> Self;
}

impl StatelessWidget for Container {
    fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
        let mut child = self.child.clone();

        // Wrap with alignment
        if let Some(alignment) = self.alignment {
            child = Some(Box::new(Align::new(alignment, child.unwrap())));
        }

        // Wrap with padding
        if let Some(padding) = self.padding {
            child = Some(Box::new(Padding::new(padding, child.unwrap())));
        }

        // Wrap with decoration
        if self.color.is_some() || self.decoration.is_some() {
            child = Some(Box::new(DecoratedBox::new(/* ... */, child.unwrap())));
        }

        // Wrap with size constraints
        if self.width.is_some() || self.height.is_some() {
            child = Some(Box::new(SizedBox::new(self.width, self.height, child)));
        }

        // Wrap with margin
        if let Some(margin) = self.margin {
            child = Some(Box::new(Padding::new(margin, child.unwrap())));
        }

        child.unwrap()
    }
}
```

**Files to create:**
- `crates/flui_widgets/src/basic/container.rs`
- `crates/flui_widgets/src/basic/sized_box.rs`
- `crates/flui_widgets/src/basic/padding.rs`
- `crates/flui_widgets/src/basic/center.rs`
- `crates/flui_widgets/src/basic/align.rs`
- `crates/flui_widgets/src/basic/decorated_box.rs`

**Time:** 5 days

---

#### 4.2 Flex Widgets (Row, Column, Expanded)

```rust
// crates/flui_widgets/src/layout/column.rs
#[derive(Debug, Clone)]
pub struct Column {
    key: Option<WidgetKey>,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    children: Vec<Box<dyn Widget>>,
}

impl Column {
    pub fn new() -> Self;
    pub fn with_children(mut self, children: Vec<Box<dyn Widget>>) -> Self;
}

impl StatelessWidget for Column {
    fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
        // Create RenderFlex with vertical axis
        Box::new(Flex::new(Axis::Vertical)
            .with_main_alignment(self.main_axis_alignment)
            .with_cross_alignment(self.cross_axis_alignment)
            .with_children(self.children.clone()))
    }
}

// Row is the same but with Axis::Horizontal

// crates/flui_widgets/src/layout/expanded.rs
#[derive(Debug, Clone)]
pub struct Expanded {
    flex: i32,
    child: Box<dyn Widget>,
}

impl Expanded {
    pub fn new(child: Box<dyn Widget>) -> Self {
        Self { flex: 1, child }
    }

    pub fn with_flex(mut self, flex: i32) -> Self {
        self.flex = flex;
        self
    }
}
```

**Files to create:**
- `crates/flui_widgets/src/layout/column.rs`
- `crates/flui_widgets/src/layout/row.rs`
- `crates/flui_widgets/src/layout/expanded.rs`
- `crates/flui_widgets/src/layout/flexible.rs`
- `crates/flui_widgets/src/layout/spacer.rs`

**Time:** 4 days

---

#### 4.3 Text Widget (Basic)

```rust
// crates/flui_widgets/src/text/text.rs
#[derive(Debug, Clone)]
pub struct Text {
    text: String,
    style: Option<TextStyle>,
    text_align: Option<egui::Align>,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self;
    pub fn with_style(mut self, style: TextStyle) -> Self;
}

// RenderText uses egui::Label for now
pub struct RenderText {
    base: RenderBox,
    text: String,
    style: Option<TextStyle>,
}

impl RenderObject for RenderText {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Measure text with egui
        // Return size that fits constraints
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint text using egui::Painter
    }
}
```

**Files to create:**
- `crates/flui_widgets/src/text/text.rs`
- `crates/flui_rendering/src/paragraph.rs`

**Tests:**
- Text layout and sizing
- Text styling
- Text alignment

**Time:** 3 days

**Total Phase 4 Time:** ~12 days

---

## 🚀 Middle-Level Roadmap

### Phase 5: Platform Integration (Week 8)

**Priority: CRITICAL**

Connect everything to egui and create the application entry point.

#### 5.1 FluiApp

```rust
// crates/flui_platform/src/app.rs
pub struct FluiApp {
    pub title: String,
    pub home: Box<dyn Widget>,
}

impl FluiApp {
    pub fn new(title: impl Into<String>, home: Box<dyn Widget>) -> Self;

    pub fn run(self) -> Result<(), eframe::Error> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(&self.title)
                .with_inner_size([800.0, 600.0]),
            ..Default::default()
        };

        eframe::run_native(
            &self.title,
            native_options,
            Box::new(|_cc| Ok(Box::new(FluiAppState::new(self)))),
        )
    }
}

struct FluiAppState {
    app: FluiApp,
    element_tree: ElementTree,
    root_id: Option<ElementId>,
}

impl eframe::App for FluiAppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request repaint for next frame
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            // 1. BUILD PHASE
            if self.root_id.is_none() {
                self.root_id = Some(self.element_tree.mount_root(self.app.home.clone()));
            }
            self.element_tree.rebuild_dirty();

            // 2. LAYOUT PHASE
            let available_size = ui.available_size();
            let constraints = BoxConstraints::tight(Size::new(
                available_size.x,
                available_size.y,
            ));

            if let Some(root_id) = self.root_id {
                let root_element = self.element_tree.get_element(root_id);
                let root_render = root_element.get_render_object();
                root_render.layout(constraints);

                // 3. PAINT PHASE
                root_render.paint(ui.painter(), Offset::ZERO);
            }
        });
    }
}
```

**Files to create:**
- `crates/flui_platform/src/app.rs`
- `crates/flui_platform/src/window.rs`

**Tests:**
- App creation and running
- Frame loop execution
- Widget tree rendering

**Time:** 5 days

---

#### 5.2 Basic Examples

Create examples to validate the framework works end-to-end.

```rust
// examples/hello_world.rs
use flui::prelude::*;

fn main() {
    FluiApp::new(
        "Hello World",
        Container::new()
            .with_color(egui::Color32::WHITE)
            .with_child(
                Center::new(
                    Text::new("Hello, Flui!")
                        .with_style(TextStyle {
                            font_size: Some(32.0),
                            color: Some(egui::Color32::BLACK),
                            ..Default::default()
                        })
                )
            )
    )
    .run()
    .unwrap();
}

// examples/counter.rs - StatefulWidget example
struct Counter;

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState { count: 0 }
    }
}

struct CounterState {
    count: i32,
}

impl State for CounterState {
    fn build(&mut self, ctx: &BuildContext) -> Box<dyn Widget> {
        Column::new()
            .with_children(vec![
                Text::new(format!("Count: {}", self.count)).into_widget(),
                // TODO: Button widget (Phase 6)
            ])
            .into_widget()
    }
}

// examples/layout_demo.rs
// Demonstrate Row, Column, Expanded, Container, etc.
```

**Examples to create:**
- `examples/hello_world.rs` - Minimal app
- `examples/counter.rs` - StatefulWidget
- `examples/layout_demo.rs` - Layout showcase
- `examples/styling_demo.rs` - Decorations and borders

**Time:** 3 days

**Total Phase 5 Time:** ~8 days

---

### Phase 6: Event Handling (Week 9-10)

**Priority: HIGH**

Add basic interactivity.

#### 6.1 GestureDetector (Basic)

```rust
// crates/flui_gestures/src/detector.rs
#[derive(Debug, Clone)]
pub struct GestureDetector {
    child: Box<dyn Widget>,
    on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub struct RenderGestureDetector {
    base: RenderProxyBox,
    on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl RenderObject for RenderGestureDetector {
    fn hit_test(&self, position: Offset) -> bool {
        // Check if position is within bounds
        let size = self.size();
        position.dx >= 0.0 && position.dx <= size.width &&
        position.dy >= 0.0 && position.dy <= size.height
    }

    fn handle_event(&self, event: &PointerEvent) {
        if let PointerEvent::Up = event {
            if let Some(on_tap) = &self.on_tap {
                on_tap();
            }
        }
    }
}
```

**Files to create:**
- `crates/flui_gestures/src/detector.rs`
- `crates/flui_gestures/src/events.rs`
- `crates/flui_gestures/src/hit_test.rs`

**Time:** 4 days

---

#### 6.2 Basic Button Widget

```rust
// crates/flui_widgets/src/material/button.rs
#[derive(Debug, Clone)]
pub struct Button {
    child: Box<dyn Widget>,
    on_pressed: Option<Arc<dyn Fn() + Send + Sync>>,
    color: Option<egui::Color32>,
}

impl StatelessWidget for Button {
    fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
        GestureDetector::new()
            .with_child(
                Container::new()
                    .with_color(self.color.unwrap_or(egui::Color32::BLUE))
                    .with_padding(EdgeInsets::symmetric(16.0, 8.0))
                    .with_decoration(BoxDecoration {
                        border_radius: Some(BorderRadius::circular(4.0)),
                        ..Default::default()
                    })
                    .with_child(self.child.clone())
            )
            .with_on_tap(self.on_pressed.clone())
            .into_widget()
    }
}
```

**Files to create:**
- `crates/flui_widgets/src/material/button.rs`

**Time:** 2 days

**Total Phase 6 Time:** ~6 days

---

### Phase 7: Animation System (Week 11-12)

**Priority: MEDIUM**

Basic animation support.

#### 7.1 Ticker & FrameScheduler

```rust
// crates/flui_scheduler/src/ticker.rs
pub struct Ticker {
    on_tick: Box<dyn FnMut(Duration)>,
    is_active: bool,
    start_time: Option<Instant>,
}

impl Ticker {
    pub fn new(on_tick: impl FnMut(Duration) + 'static) -> Self;
    pub fn start(&mut self);
    pub fn stop(&mut self);
    pub fn tick(&mut self, elapsed: Duration);
}

// crates/flui_scheduler/src/scheduler.rs
pub struct FrameScheduler {
    tickers: Vec<Ticker>,
    frame_callbacks: Vec<Box<dyn FnOnce()>>,
}

impl FrameScheduler {
    pub fn schedule_frame(&mut self, callback: impl FnOnce() + 'static);
    pub fn create_ticker(&mut self, on_tick: impl FnMut(Duration) + 'static) -> TickerId;
    pub fn tick_frame(&mut self, elapsed: Duration);
}
```

**Files to create:**
- `crates/flui_scheduler/src/ticker.rs`
- `crates/flui_scheduler/src/scheduler.rs`

**Time:** 3 days

---

#### 7.2 AnimationController

```rust
// crates/flui_animation/src/controller.rs
pub struct AnimationController {
    value: f64,                    // 0.0 to 1.0
    duration: Duration,
    status: AnimationStatus,
    ticker: Option<Ticker>,
    listeners: Vec<Box<dyn Fn(f64)>>,
}

pub enum AnimationStatus {
    Dismissed,
    Forward,
    Reverse,
    Completed,
}

impl AnimationController {
    pub fn new(duration: Duration) -> Self;
    pub fn forward(&mut self);
    pub fn reverse(&mut self);
    pub fn reset(&mut self);
    pub fn add_listener(&mut self, listener: impl Fn(f64) + 'static);
}
```

**Files to create:**
- `crates/flui_animation/src/controller.rs`
- `crates/flui_animation/src/status.rs`

**Time:** 3 days

---

#### 7.3 Tweens & Curves

```rust
// crates/flui_animation/src/tween.rs
pub trait Animatable<T> {
    fn lerp(&self, t: f64) -> T;
}

pub struct Tween<T> {
    begin: T,
    end: T,
}

impl<T: Lerp> Animatable<T> for Tween<T> {
    fn lerp(&self, t: f64) -> T {
        T::lerp(&self.begin, &self.end, t)
    }
}

// crates/flui_animation/src/curves.rs
pub trait Curve {
    fn transform(&self, t: f64) -> f64;
}

pub struct Linear;
pub struct EaseIn;
pub struct EaseOut;
pub struct EaseInOut;
```

**Files to create:**
- `crates/flui_animation/src/tween.rs`
- `crates/flui_animation/src/curves.rs`

**Time:** 2 days

**Total Phase 7 Time:** ~8 days

---

## 📦 Dependencies

### Core Dependencies

```toml
[dependencies]
# Rendering
egui = "0.33"
eframe = "0.33"

# Concurrency
parking_lot = "0.12"        # Fast Mutex/RwLock

# Serialization
serde = { version = "1.0", features = ["derive"] }

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"
```

### Optional Dependencies (Future)

```toml
# Math (for advanced transforms)
glam = { version = "0.29", features = ["serde"], optional = true }

# Async (for futures)
tokio = { version = "1.40", features = ["sync", "time"], optional = true }

# Collections (for optimizations)
smallvec = "1.13"
indexmap = "2.5"
```

---

## 📊 Success Metrics

### Low-Level Goals

- [ ] All 4 foundation crates at 100% completeness
- [ ] RenderFlex passes Flutter layout conformance tests
- [ ] RenderStack supports all positioning combinations
- [ ] Painting system renders borders, shadows, decorations correctly
- [ ] Element tree handles 1000+ elements efficiently

### Middle-Level Goals

- [ ] FluiApp runs and displays widgets
- [ ] Counter example works (StatefulWidget + setState)
- [ ] Layout demo shows complex nested layouts
- [ ] Button responds to clicks
- [ ] Basic animations run smoothly at 60fps

### Code Quality

- [ ] 80%+ test coverage for core crates
- [ ] All public APIs documented with rustdoc
- [ ] Zero clippy warnings
- [ ] CI passes on all platforms (Windows, Linux, macOS)

---

## 📝 Timeline Summary

| Phase | Focus | Duration | Status |
|-------|-------|----------|--------|
| 0 | Project Setup | ✅ Complete | ✅ |
| 1 | Foundation Types | ✅ Complete | ✅ |
| 2 | Core Traits | ✅ Complete | ✅ |
| **1** | **Painting System** | **10 days** | ⏳ Next |
| **2** | **Layout System** | **10 days** | ⏳ |
| **3** | **Element Tree** | **9 days** | ⏳ |
| **4** | **Basic Widgets** | **12 days** | ⏳ |
| **5** | **Platform Integration** | **8 days** | ⏳ |
| **6** | **Event Handling** | **6 days** | ⏳ |
| **7** | **Animation System** | **8 days** | ⏳ |

**Total Time for Low/Mid-Level:** ~9 weeks

---

## 🎯 Next Actions (Prioritized)

### Immediate Priority (Week 1-2): Layout System

**Goal:** Enable Row/Column layout - the foundation of Flutter layouts

1. **RenderFlex Implementation** (5 days)
   - Create `crates/flui_rendering/src/flex.rs`
   - Implement layout algorithm (measure inflexible → distribute flex space → position)
   - Add FlexParentData for flex factors
   - Test with various alignments (Start, End, Center, SpaceBetween, etc.)
   - File: ~400 lines, 15+ tests

2. **Row/Column Widgets** (2 days)
   - Create `crates/flui_widgets/src/layout/row.rs`
   - Create `crates/flui_widgets/src/layout/column.rs`
   - Create `crates/flui_widgets/src/layout/expanded.rs`
   - Simple wrappers around RenderFlex
   - File: ~150 lines each, 10+ tests

3. **RenderPadding** (1 day)
   - Create `crates/flui_rendering/src/padding.rs`
   - Simple constraint deflation/inflation
   - File: ~100 lines, 5+ tests

### Secondary Priority (Week 3): Basic Widgets

4. **Container Widget** (3 days)
   - Needs EdgeInsets from flui_types (already done!)
   - Simple composition of SizedBox + Padding + DecoratedBox
   - File: ~200 lines, 8+ tests

5. **SizedBox & Center** (1 day)
   - Fixed size constraints
   - Alignment wrapper
   - File: ~100 lines each, 5+ tests

### Tertiary Priority (Week 4): Painting

6. **Basic BoxDecoration** (2 days)
   - Color filling
   - Border rendering
   - No shadows/gradients yet
   - File: ~150 lines, 8+ tests

---

## 📚 References

- [Flutter Architecture](https://docs.flutter.dev/resources/architectural-overview)
- [egui Documentation](https://docs.rs/egui/0.33/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)

---

**Last Updated:** 2025-01-18 (RenderDecoratedBox + RenderAspectRatio Complete!)
**Version:** 0.1.0-alpha
**Phase:** Layout System Implementation 🚧 **IN PROGRESS**
**Next Phase:** Additional Layout RenderObjects (RenderLimitedBox, RenderIndexedStack, RenderWrap)
**Next Review:** After completing 10+ RenderObjects

---

## 🎊 Major Milestones Achieved!

### ✅ Core Infrastructure (100% Complete)
**flui_core** - Full three-tree architecture:
- ✅ Widget → Element → RenderObject pipeline fully implemented
- ✅ All traits use modern Rust patterns (DynClone, DowncastSync)
- ✅ RenderObjectElement manages render object lifecycle
- ✅ InheritedWidget for efficient data propagation
- ✅ ParentData system for layout information
- ✅ 49 comprehensive tests covering all core functionality

### 🚧 Layout System (60% Complete)
**flui_rendering** - Essential RenderObjects:
- ✅ RenderFlex - Row/Column layout with flexible children (15 tests)
- ✅ RenderPadding - Padding layout with EdgeInsets (8 tests)
- ✅ RenderStack - Positioned layout with StackFit (13 tests)
- ✅ RenderConstrainedBox - Additional constraints (10 tests)
- ✅ RenderDecoratedBox - BoxDecoration painting (10 tests) **NEW!**
- ✅ RenderAspectRatio - Aspect ratio support (17 tests) **NEW!**
- ✅ BoxDecorationPainter - Stateful painter for decorations (6 tests) **NEW!**

### 📊 Today's Progress (2025-01-18)
- **+2 RenderObjects** (RenderDecoratedBox, RenderAspectRatio)
- **+17 tests** (было 82, стало 99 в flui_rendering)
- **+890 строк кода**

**Total test count:** 701 tests (525 flui_types + 49 flui_core + 99 flui_rendering + 27 flui_animation + 1 flui_foundation)

Ready for more RenderObjects! 🚀
