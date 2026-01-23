# Phase 6: Render Tree Layer (flui_rendering) - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущие фазы**: Phase 1-5 должны быть завершены  
> **Референсы**: `.flutter/src/rendering/`, Flutter RenderObject system, GPUI scene graph  
> **Цель**: Production-ready RenderObject system с layout, paint, hit testing и type-safe arity

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui_rendering
- ✅ Cargo.toml с зависимостями
- ✅ Модульная структура: `objects/`, `pipeline/`, `protocol/`, `constraints/`, `hit_testing/`
- ✅ Arity system: `Leaf`, `Single`, `Optional`, `Variable`
- ✅ BoxChild container для type-safe children
- ✅ Базовые RenderObject traits
- ✅ Ambassador delegation pattern
- ✅ Slab-based storage
- ✅ PipelineOwner basics

#### Зависимости готовы
- ✅ flui_types - geometry, constraints
- ✅ flui_painting - Paint, Canvas abstractions
- ✅ flui-foundation - базовые utilities
- ✅ flui-tree - tree abstractions
- ✅ flui-layer - compositing layers
- ✅ flui-semantics - accessibility

### ❌ Что Нужно Доделать / Улучшить

#### Core RenderObject System
1. **RenderObject Lifecycle** - attach, detach, layout, paint
2. **PipelineOwner** - manages render tree, schedules layout/paint
3. **ParentData** - child-specific layout data
4. **Constraints** - BoxConstraints, FlexConstraints, etc.
5. **Intrinsic Sizing** - min/max intrinsic width/height

#### Layout System
1. **Two-phase Layout** - parent sets constraints → child returns size
2. **Layout Protocol** - performLayout implementation
3. **Baseline Protocol** - text baseline alignment
4. **Relayout Boundary** - layout optimization

#### Paint System
1. **PaintContext** - painting infrastructure
2. **Layer System** - compositing layers for GPU
3. **Paint Protocol** - paint implementation
4. **Repaint Boundary** - paint optimization

#### Hit Testing
1. **HitTestResult** - pointer hit test results
2. **HitTestTarget** - elements that can be hit
3. **Transform Handling** - coordinate space transforms

---

## Детальный План Реализации

### Этап 6.1: Core RenderObject Architecture (Неделя 11, Дни 1-3)

#### День 1: RenderObject Trait & Lifecycle

**Цель**: Define core RenderObject trait with full lifecycle

**Референсы**:
- `.flutter/src/rendering/object.dart` - Flutter RenderObject
- Plan `3.8 Render Tree Architecture`
- FLUI existing `flui_rendering/src/traits/render_object.rs`

**Задачи**:

1. **Обновить `traits/render_object.rs`**
   ```rust
   use ambassador::delegatable_trait;
   use flui_types::*;
   use flui_painting::*;
   
   /// RenderObject trait (base for all render objects)
   ///
   /// RenderObjects form the render tree and are responsible for:
   /// - Layout (computing size and position)
   /// - Painting (drawing to canvas)
   /// - Hit testing (finding what's under a pointer)
   /// - Semantics (accessibility)
   ///
   /// # Lifecycle
   ///
   /// 1. **Attach**: RenderObject is added to tree
   /// 2. **Layout**: Parent sets constraints, object computes size
   /// 3. **Paint**: Object draws itself and children
   /// 4. **Detach**: RenderObject is removed from tree
   ///
   /// # Design Pattern
   ///
   /// RenderObjects are mutable and long-lived (unlike Views).
   /// They cache layout results and only recompute when marked dirty.
   #[delegatable_trait]
   pub trait RenderObject: 'static + Send {
       /// Attach to the render tree
       ///
       /// Called when this render object is added to the tree.
       /// Subclasses must call attach on all children.
       fn attach(&mut self, owner: PipelineOwner) {
           // Default: do nothing
       }
       
       /// Detach from the render tree
       ///
       /// Called when this render object is removed from the tree.
       /// Subclasses must call detach on all children.
       fn detach(&mut self) {
           // Default: do nothing
       }
       
       /// Mark this render object as needing layout
       fn mark_needs_layout(&mut self);
       
       /// Mark this render object as needing paint
       fn mark_needs_paint(&mut self);
       
       /// Mark this render object as needing semantics update
       fn mark_needs_semantics_update(&mut self) {
           // Default: do nothing
       }
       
       /// Check if this render object needs layout
       fn needs_layout(&self) -> bool;
       
       /// Check if this render object needs paint
       fn needs_paint(&self) -> bool;
       
       /// Perform layout with given constraints
       ///
       /// Subclasses must:
       /// 1. Respect constraints
       /// 2. Lay out children
       /// 3. Set their own size
       fn layout(&mut self, constraints: Constraints);
       
       /// Paint this render object
       ///
       /// Called during the paint phase. Subclasses must:
       /// 1. Paint themselves
       /// 2. Paint their children (using paint_child)
       fn paint(&self, context: &mut PaintContext, offset: Offset);
       
       /// Perform hit test
       ///
       /// Returns true if this render object or any of its children
       /// are hit by the given position.
       fn hit_test(&self, result: &mut HitTestResult, position: Point) -> bool {
           false // Default: not hittable
       }
       
       /// Get parent
       fn parent(&self) -> Option<RenderObjectId> {
           None
       }
       
       /// Set parent
       fn set_parent(&mut self, parent: Option<RenderObjectId>) {
           // Default: do nothing
       }
       
       /// Visit children
       fn visit_children(&self, visitor: &mut dyn FnMut(RenderObjectId)) {
           // Default: no children
       }
       
       /// Get size (if laid out)
       fn size(&self) -> Option<Size> {
           None
       }
       
       /// Type name for debugging
       fn type_name(&self) -> &'static str {
           std::any::type_name::<Self>()
       }
   }
   
   /// Type-erased render object
   pub trait AnyRenderObject: RenderObject {
       /// Downcast to concrete type
       fn as_any(&self) -> &dyn Any;
       
       /// Downcast to concrete type (mutable)
       fn as_any_mut(&mut self) -> &mut dyn Any;
   }
   
   impl<T: RenderObject> AnyRenderObject for T {
       fn as_any(&self) -> &dyn Any {
           self
       }
       
       fn as_any_mut(&mut self) -> &mut dyn Any {
           self
       }
   }
   ```

2. **PipelineOwner Basics (обновить `pipeline/owner.rs`)**
   ```rust
   use parking_lot::RwLock;
   use slab::Slab;
   use std::sync::Arc;
   use dashmap::DashMap;
   
   /// Pipeline owner
   ///
   /// Manages the render tree and schedules layout and paint phases.
   pub struct PipelineOwner {
       /// Render object storage
       objects: Arc<RwLock<Slab<Box<dyn AnyRenderObject>>>>,
       
       /// Nodes that need layout
       nodes_needing_layout: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Nodes that need paint
       nodes_needing_paint: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Nodes that need semantics update
       nodes_needing_semantics: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Root render object
       root: Arc<RwLock<Option<RenderObjectId>>>,
   }
   
   impl PipelineOwner {
       pub fn new() -> Self {
           Self {
               objects: Arc::new(RwLock::new(Slab::new())),
               nodes_needing_layout: Arc::new(DashMap::new()),
               nodes_needing_paint: Arc::new(DashMap::new()),
               nodes_needing_semantics: Arc::new(DashMap::new()),
               root: Arc::new(RwLock::new(None)),
           }
       }
       
       /// Insert a render object
       pub fn insert(&self, object: Box<dyn AnyRenderObject>) -> RenderObjectId {
           let mut objects = self.objects.write();
           let index = objects.insert(object);
           RenderObjectId::new(index + 1) // +1 for NonZeroUsize
       }
       
       /// Get render object
       pub fn get(&self, id: RenderObjectId) -> Option<impl Deref<Target = Box<dyn AnyRenderObject>> + '_> {
           let objects = self.objects.read();
           if objects.len() >= id.get() {
               Some(RwLockReadGuard::map(objects, |o| &o[id.get() - 1]))
           } else {
               None
           }
       }
       
       /// Get render object (mutable)
       pub fn get_mut(&self, id: RenderObjectId) -> Option<impl DerefMut<Target = Box<dyn AnyRenderObject>> + '_> {
           let objects = self.objects.write();
           if objects.len() >= id.get() {
               Some(RwLockWriteGuard::map(objects, |o| &mut o[id.get() - 1]))
           } else {
               None
           }
       }
       
       /// Set root
       pub fn set_root(&self, id: RenderObjectId) {
           *self.root.write() = Some(id);
       }
       
       /// Request layout for a node
       pub fn request_layout(&self, id: RenderObjectId) {
           self.nodes_needing_layout.insert(id, ());
       }
       
       /// Request paint for a node
       pub fn request_paint(&self, id: RenderObjectId) {
           self.nodes_needing_paint.insert(id, ());
       }
       
       /// Flush layout (perform layout for all dirty nodes)
       pub fn flush_layout(&self) {
           tracing::debug!("Flushing layout for {} nodes", self.nodes_needing_layout.len());
           
           // Collect and sort by depth (top-down)
           let mut dirty: Vec<RenderObjectId> = self.nodes_needing_layout
               .iter()
               .map(|entry| *entry.key())
               .collect();
           
           self.nodes_needing_layout.clear();
           
           dirty.sort_by_key(|&id| self.depth(id));
           
           // Layout each node
           for id in dirty {
               if let Some(mut object) = self.get_mut(id) {
                   if object.needs_layout() {
                       // Get constraints from parent
                       let constraints = self.get_constraints(id);
                       object.layout(constraints);
                   }
               }
           }
       }
       
       /// Flush paint (paint all dirty nodes)
       pub fn flush_paint(&self) -> Scene {
           tracing::debug!("Flushing paint for {} nodes", self.nodes_needing_paint.len());
           
           let mut context = PaintContext::new();
           
           // Paint from root
           if let Some(root_id) = *self.root.read() {
               if let Some(root) = self.get(root_id) {
                   root.paint(&mut context, Offset::zero());
               }
           }
           
           context.into_scene()
       }
       
       fn depth(&self, id: RenderObjectId) -> usize {
           let mut depth = 0;
           let mut current = id;
           
           while let Some(object) = self.get(current) {
               if let Some(parent_id) = object.parent() {
                   depth += 1;
                   current = parent_id;
               } else {
                   break;
               }
           }
           
           depth
       }
       
       fn get_constraints(&self, id: RenderObjectId) -> Constraints {
           // TODO: Get constraints from parent's layout
           Constraints::default()
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderObject trait complete
- [ ] PipelineOwner manages tree
- [ ] Attach/detach lifecycle works
- [ ] 30+ render object tests

---

#### День 2: BoxProtocol & RenderBox

**Цель**: Implement box-based layout protocol

**Задачи**:

1. **RenderBox Trait (создать `protocol/box_protocol.rs`)**
   ```rust
   use flui_types::*;
   
   /// Box protocol (2D rectangular layout)
   ///
   /// Most UI elements use box layout. Children are given
   /// BoxConstraints and must return a Size.
   ///
   /// # Type-Safe Arity
   ///
   /// RenderBox is generic over Arity to enforce child count at compile-time:
   /// - `RenderBox<Leaf>`: No children
   /// - `RenderBox<Single>`: Exactly one child
   /// - `RenderBox<Optional>`: Zero or one child
   /// - `RenderBox<Variable>`: N children
   pub trait RenderBox<A: Arity>: RenderObject {
       /// Perform layout
       ///
       /// Called by the parent during layout. Must:
       /// 1. Lay out children (if any)
       /// 2. Compute own size
       /// 3. Position children (via ParentData)
       fn perform_layout(&mut self, constraints: BoxConstraints);
       
       /// Get computed size
       fn size(&self) -> Size;
       
       /// Set size
       fn set_size(&mut self, size: Size);
       
       /// Compute minimum intrinsic width
       ///
       /// The minimum width this box could be painted at for a given height.
       fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
           0.0 // Default: 0
       }
       
       /// Compute maximum intrinsic width
       fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
           0.0 // Default: 0
       }
       
       /// Compute minimum intrinsic height
       fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
           0.0
       }
       
       /// Compute maximum intrinsic height
       fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
           0.0
       }
       
       /// Compute distance to baseline
       ///
       /// Used for aligning text with different font sizes.
       fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
           None // Default: no baseline
       }
   }
   
   /// Box constraints
   #[derive(Copy, Clone, Debug, PartialEq)]
   pub struct BoxConstraints {
       pub min_width: f32,
       pub max_width: f32,
       pub min_height: f32,
       pub max_height: f32,
   }
   
   impl BoxConstraints {
       /// Create tight constraints (exact size)
       pub fn tight(size: Size) -> Self {
           Self {
               min_width: size.width,
               max_width: size.width,
               min_height: size.height,
               max_height: size.height,
           }
       }
       
       /// Create loose constraints (max size)
       pub fn loose(size: Size) -> Self {
           Self {
               min_width: 0.0,
               max_width: size.width,
               min_height: 0.0,
               max_height: size.height,
           }
       }
       
       /// Create expand constraints (fill available space)
       pub fn expand() -> Self {
           Self {
               min_width: f32::INFINITY,
               max_width: f32::INFINITY,
               min_height: f32::INFINITY,
               max_height: f32::INFINITY,
           }
       }
       
       /// Constrain a size to these constraints
       pub fn constrain(&self, size: Size) -> Size {
           Size::new(
               size.width.clamp(self.min_width, self.max_width),
               size.height.clamp(self.min_height, self.max_height),
           )
       }
       
       /// Check if constraints are tight
       pub fn is_tight(&self) -> bool {
           self.min_width == self.max_width && self.min_height == self.max_height
       }
       
       /// Get biggest size that satisfies constraints
       pub fn biggest(&self) -> Size {
           Size::new(self.max_width, self.max_height)
       }
       
       /// Get smallest size that satisfies constraints
       pub fn smallest(&self) -> Size {
           Size::new(self.min_width, self.min_height)
       }
       
       /// Tighten constraints
       pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
           Self {
               min_width: width.unwrap_or(self.min_width),
               max_width: width.unwrap_or(self.max_width),
               min_height: height.unwrap_or(self.min_height),
               max_height: height.unwrap_or(self.max_height),
           }
       }
       
       /// Loosen constraints
       pub fn loosen(&self) -> Self {
           Self {
               min_width: 0.0,
               max_width: self.max_width,
               min_height: 0.0,
               max_height: self.max_height,
           }
       }
   }
   ```

2. **Example: RenderPadding (Single Child)**
   ```rust
   use ambassador::Delegate;
   
   /// Render padding (adds padding around child)
   #[derive(Delegate)]
   #[delegate(RenderObject, target = "child")]
   pub struct RenderPadding {
       /// Child (type-safe: exactly one)
       child: BoxChild<Single>,
       
       /// Padding
       padding: EdgeInsets,
       
       /// Computed size
       size: Size,
       
       /// Dirty flags
       needs_layout: bool,
       needs_paint: bool,
   }
   
   impl RenderPadding {
       pub fn new(padding: EdgeInsets) -> Self {
           Self {
               child: BoxChild::new(),
               padding,
               size: Size::zero(),
               needs_layout: true,
               needs_paint: true,
           }
       }
       
       pub fn set_padding(&mut self, padding: EdgeInsets) {
           if self.padding != padding {
               self.padding = padding;
               self.mark_needs_layout();
           }
       }
       
       pub fn set_child(&mut self, child: RenderObjectId) {
           self.child.set_child(child);
           self.mark_needs_layout();
       }
   }
   
   impl RenderObject for RenderPadding {
       fn attach(&mut self, owner: PipelineOwner) {
           self.child.attach(owner.clone());
       }
       
       fn detach(&mut self) {
           self.child.detach();
       }
       
       fn mark_needs_layout(&mut self) {
           self.needs_layout = true;
           // Propagate up
       }
       
       fn mark_needs_paint(&mut self) {
           self.needs_paint = true;
           // Propagate up
       }
       
       fn needs_layout(&self) -> bool {
           self.needs_layout
       }
       
       fn needs_paint(&self) -> bool {
           self.needs_paint
       }
       
       fn layout(&mut self, constraints: Constraints) {
           let box_constraints = constraints.as_box().unwrap();
           self.perform_layout(box_constraints);
           self.needs_layout = false;
       }
       
       fn paint(&self, context: &mut PaintContext, offset: Offset) {
           // Paint child with padding offset
           let child_offset = offset + Offset::new(
               self.padding.left,
               self.padding.top,
           );
           
           self.child.paint(context, child_offset);
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(RenderObjectId)) {
           self.child.visit_children(visitor);
       }
       
       fn size(&self) -> Option<Size> {
           Some(self.size)
       }
   }
   
   impl RenderBox<Single> for RenderPadding {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // Compute child constraints (subtract padding)
           let child_constraints = BoxConstraints {
               min_width: (constraints.min_width - self.padding.horizontal()).max(0.0),
               max_width: (constraints.max_width - self.padding.horizontal()).max(0.0),
               min_height: (constraints.min_height - self.padding.vertical()).max(0.0),
               max_height: (constraints.max_height - self.padding.vertical()).max(0.0),
           };
           
           // Layout child
           let child_size = self.child.layout(child_constraints);
           
           // Compute own size (child + padding)
           self.size = Size::new(
               child_size.width + self.padding.horizontal(),
               child_size.height + self.padding.vertical(),
           );
           
           // Constrain to parent constraints
           self.size = constraints.constrain(self.size);
       }
       
       fn size(&self) -> Size {
           self.size
       }
       
       fn set_size(&mut self, size: Size) {
           self.size = size;
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderBox trait works
- [ ] BoxConstraints works
- [ ] RenderPadding example works
- [ ] Intrinsic sizing works
- [ ] 35+ box protocol tests

---

#### День 3: ParentData & Child Positioning

**Цель**: Parent-specific data for each child

**Задачи**:

1. **ParentData Trait**
   ```rust
   /// Parent data (child-specific layout information)
   ///
   /// Each parent can store custom data on its children
   /// for layout purposes (position, flex factor, etc.)
   pub trait ParentData: 'static + Send {
       /// Detach from child
       fn detach(&mut self) {
           // Default: do nothing
       }
   }
   
   /// Box parent data (position)
   #[derive(Default, Clone, Debug)]
   pub struct BoxParentData {
       /// Offset from parent's top-left
       pub offset: Offset,
   }
   
   impl ParentData for BoxParentData {}
   
   /// Flex parent data (for Row/Column children)
   #[derive(Clone, Debug)]
   pub struct FlexParentData {
       /// Base box data
       pub box_data: BoxParentData,
       
       /// Flex factor (0 = non-flexible)
       pub flex: u32,
       
       /// Flex fit (tight or loose)
       pub fit: FlexFit,
   }
   
   impl ParentData for FlexParentData {}
   
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum FlexFit {
       /// Child can be smaller than flex space
       Loose,
       
       /// Child must fill flex space
       Tight,
   }
   ```

2. **Setup ParentData**
   ```rust
   impl RenderPadding {
       fn setup_parent_data(&self, child_id: RenderObjectId, owner: &PipelineOwner) {
           if let Some(mut child) = owner.get_mut(child_id) {
               // Ensure child has BoxParentData
               if child.parent_data::<BoxParentData>().is_none() {
                   child.set_parent_data(Box::new(BoxParentData::default()));
               }
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] ParentData trait works
- [ ] BoxParentData works
- [ ] FlexParentData works
- [ ] Child positioning works
- [ ] 25+ parent data tests

---

### Этап 6.2: Layout Implementation (Неделя 11-12, Дни 4-6)

#### День 4: Leaf RenderObjects (Text, Image)

**Цель**: Implement leaf render objects (no children)

**Задачи**:

1. **RenderText (Leaf)**
   ```rust
   /// Render text (leaf)
   pub struct RenderText {
       /// Text content
       text: String,
       
       /// Text style
       style: TextStyle,
       
       /// Computed size
       size: Size,
       
       /// Text layout (from text system)
       text_layout: Option<TextLayout>,
       
       /// Dirty flags
       needs_layout: bool,
       needs_paint: bool,
   }
   
   impl RenderText {
       pub fn new(text: String, style: TextStyle) -> Self {
           Self {
               text,
               style,
               size: Size::zero(),
               text_layout: None,
               needs_layout: true,
               needs_paint: true,
           }
       }
       
       pub fn set_text(&mut self, text: String) {
           if self.text != text {
               self.text = text;
               self.mark_needs_layout();
           }
       }
       
       pub fn set_style(&mut self, style: TextStyle) {
           if self.style != style {
               self.style = style;
               self.mark_needs_layout();
           }
       }
   }
   
   impl RenderObject for RenderText {
       fn mark_needs_layout(&mut self) {
           self.needs_layout = true;
       }
       
       fn mark_needs_paint(&mut self) {
           self.needs_paint = true;
       }
       
       fn needs_layout(&self) -> bool {
           self.needs_layout
       }
       
       fn needs_paint(&self) -> bool {
           self.needs_paint
       }
       
       fn layout(&mut self, constraints: Constraints) {
           let box_constraints = constraints.as_box().unwrap();
           self.perform_layout(box_constraints);
           self.needs_layout = false;
       }
       
       fn paint(&self, context: &mut PaintContext, offset: Offset) {
           if let Some(layout) = &self.text_layout {
               context.draw_text(layout, offset, self.style.color);
           }
       }
       
       fn size(&self) -> Option<Size> {
           Some(self.size)
       }
   }
   
   impl RenderBox<Leaf> for RenderText {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // Layout text
           let text_system = TextSystem::global();
           self.text_layout = Some(text_system.layout_text(
               &self.text,
               &self.style,
               constraints.max_width,
           ));
           
           // Get size from layout
           if let Some(layout) = &self.text_layout {
               self.size = layout.size();
           } else {
               self.size = Size::zero();
           }
           
           // Constrain to box constraints
           self.size = constraints.constrain(self.size);
       }
       
       fn size(&self) -> Size {
           self.size
       }
       
       fn set_size(&mut self, size: Size) {
           self.size = size;
       }
       
       fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
           self.text_layout.as_ref()
               .and_then(|layout| layout.baseline(baseline))
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderText works
- [ ] Text layout works
- [ ] Baseline alignment works
- [ ] 30+ text rendering tests

---

#### День 5: Container RenderObjects (Padding, Center, SizedBox)

**Цель**: Single-child containers

**Задачи**:

1. **RenderCenter (Single)**
   ```rust
   /// Render center (centers child)
   #[derive(Delegate)]
   #[delegate(RenderObject, target = "child")]
   pub struct RenderCenter {
       child: BoxChild<Single>,
       size: Size,
       needs_layout: bool,
       needs_paint: bool,
   }
   
   impl RenderBox<Single> for RenderCenter {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // Layout child with loose constraints
           let child_constraints = constraints.loosen();
           let child_size = self.child.layout(child_constraints);
           
           // Take full available space
           self.size = constraints.biggest();
           
           // Center child
           let child_offset = Offset::new(
               (self.size.width - child_size.width) / 2.0,
               (self.size.height - child_size.height) / 2.0,
           );
           
           // Set child position via ParentData
           if let Some(child_id) = self.child.child_id() {
               if let Some(mut child) = self.child.owner().get_mut(child_id) {
                   if let Some(parent_data) = child.parent_data_mut::<BoxParentData>() {
                       parent_data.offset = child_offset;
                   }
               }
           }
       }
       
       fn size(&self) -> Size {
           self.size
       }
       
       fn set_size(&mut self, size: Size) {
           self.size = size;
       }
   }
   ```

2. **RenderSizedBox (Optional)**
   ```rust
   /// Render sized box (fixed or constrained size)
   #[derive(Delegate)]
   #[delegate(RenderObject, target = "child")]
   pub struct RenderSizedBox {
       child: BoxChild<Optional>,
       width: Option<f32>,
       height: Option<f32>,
       size: Size,
       needs_layout: bool,
       needs_paint: bool,
   }
   
   impl RenderBox<Optional> for RenderSizedBox {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // Compute own constraints
           let width = self.width.unwrap_or(constraints.max_width);
           let height = self.height.unwrap_or(constraints.max_height);
           
           self.size = constraints.constrain(Size::new(width, height));
           
           // Layout child (if any) with tight constraints
           if self.child.has_child() {
               let child_constraints = BoxConstraints::tight(self.size);
               self.child.layout(child_constraints);
           }
       }
       
       fn size(&self) -> Size {
           self.size
       }
       
       fn set_size(&mut self, size: Size) {
           self.size = size;
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderCenter works
- [ ] RenderSizedBox works
- [ ] Child positioning works
- [ ] 30+ container tests

---

#### День 6: Flex Layout (Row, Column)

**Цель**: Variable-child flex layout

**Задачи**:

1. **RenderFlex (Variable)**
   ```rust
   /// Render flex (Row or Column)
   #[derive(Delegate)]
   #[delegate(RenderObject, target = "children")]
   pub struct RenderFlex {
       /// Children (type-safe: N children)
       children: BoxChild<Variable>,
       
       /// Flex direction
       direction: Axis,
       
       /// Main axis alignment
       main_axis_alignment: MainAxisAlignment,
       
       /// Cross axis alignment
       cross_axis_alignment: CrossAxisAlignment,
       
       /// Main axis size
       main_axis_size: MainAxisSize,
       
       /// Computed size
       size: Size,
       
       /// Dirty flags
       needs_layout: bool,
       needs_paint: bool,
   }
   
   impl RenderBox<Variable> for RenderFlex {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // 1. Layout non-flexible children
           let mut total_flex = 0u32;
           let mut allocated_size = 0.0;
           let mut max_cross_size = 0.0;
           
           for child_id in self.children.child_ids() {
               let parent_data = self.get_child_parent_data::<FlexParentData>(child_id);
               
               if parent_data.flex == 0 {
                   // Non-flexible: layout with loose constraints
                   let child_constraints = self.make_child_constraints(
                       constraints,
                       None, // no specific main axis size
                   );
                   
                   let child_size = self.layout_child(child_id, child_constraints);
                   
                   allocated_size += self.main_size(child_size);
                   max_cross_size = max_cross_size.max(self.cross_size(child_size));
               } else {
                   total_flex += parent_data.flex;
               }
           }
           
           // 2. Distribute remaining space to flexible children
           let free_space = self.main_axis_size(constraints) - allocated_size;
           let space_per_flex = if total_flex > 0 {
               free_space.max(0.0) / total_flex as f32
           } else {
               0.0
           };
           
           for child_id in self.children.child_ids() {
               let parent_data = self.get_child_parent_data::<FlexParentData>(child_id);
               
               if parent_data.flex > 0 {
                   let child_main_size = space_per_flex * parent_data.flex as f32;
                   
                   let child_constraints = self.make_child_constraints(
                       constraints,
                       Some(child_main_size),
                   );
                   
                   let child_size = self.layout_child(child_id, child_constraints);
                   max_cross_size = max_cross_size.max(self.cross_size(child_size));
               }
           }
           
           // 3. Position children
           let mut main_position = self.compute_main_start_position(
               constraints,
               allocated_size + total_flex as f32 * space_per_flex,
           );
           
           for child_id in self.children.child_ids() {
               let child_size = self.get_child_size(child_id);
               let cross_position = self.compute_cross_position(
                   max_cross_size,
                   self.cross_size(child_size),
               );
               
               // Set child position
               let offset = if self.direction == Axis::Horizontal {
                   Offset::new(main_position, cross_position)
               } else {
                   Offset::new(cross_position, main_position)
               };
               
               self.set_child_parent_data(child_id, offset);
               
               main_position += self.main_size(child_size);
           }
           
           // 4. Compute own size
           self.size = if self.direction == Axis::Horizontal {
               Size::new(allocated_size, max_cross_size)
           } else {
               Size::new(max_cross_size, allocated_size)
           };
           
           self.size = constraints.constrain(self.size);
       }
       
       fn size(&self) -> Size {
           self.size
       }
       
       fn set_size(&mut self, size: Size) {
           self.size = size;
       }
   }
   
   impl RenderFlex {
       fn main_size(&self, size: Size) -> f32 {
           match self.direction {
               Axis::Horizontal => size.width,
               Axis::Vertical => size.height,
           }
       }
       
       fn cross_size(&self, size: Size) -> f32 {
           match self.direction {
               Axis::Horizontal => size.height,
               Axis::Vertical => size.width,
           }
       }
       
       fn main_axis_size(&self, constraints: BoxConstraints) -> f32 {
           match self.direction {
               Axis::Horizontal => constraints.max_width,
               Axis::Vertical => constraints.max_height,
           }
       }
       
       fn compute_main_start_position(&self, constraints: BoxConstraints, total_size: f32) -> f32 {
           let available = self.main_axis_size(constraints);
           let free_space = (available - total_size).max(0.0);
           
           match self.main_axis_alignment {
               MainAxisAlignment::Start => 0.0,
               MainAxisAlignment::End => free_space,
               MainAxisAlignment::Center => free_space / 2.0,
               MainAxisAlignment::SpaceBetween => 0.0,
               MainAxisAlignment::SpaceAround => free_space / (self.children.len() as f32 * 2.0),
               MainAxisAlignment::SpaceEvenly => free_space / (self.children.len() as f32 + 1.0),
           }
       }
       
       fn compute_cross_position(&self, max_cross: f32, child_cross: f32) -> f32 {
           match self.cross_axis_alignment {
               CrossAxisAlignment::Start => 0.0,
               CrossAxisAlignment::End => max_cross - child_cross,
               CrossAxisAlignment::Center => (max_cross - child_cross) / 2.0,
               CrossAxisAlignment::Stretch => 0.0,
               CrossAxisAlignment::Baseline => {
                   // TODO: Baseline alignment
                   0.0
               }
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderFlex works (Row/Column)
- [ ] Flex factors work
- [ ] Main axis alignment works
- [ ] Cross axis alignment works
- [ ] 40+ flex layout tests

---

### Этап 6.3: Paint & Hit Testing (Неделя 12, Дні 7-10)

#### День 7: Paint System & Layers

**Цель**: Implement painting to layers

**Задачи**:

1. **PaintContext**
   ```rust
   /// Paint context
   ///
   /// Provides painting infrastructure (canvas, layers, transforms).
   pub struct PaintContext {
       /// Layer tree builder
       layer_builder: LayerTreeBuilder,
       
       /// Current transform
       transform: Transform2D,
       
       /// Current clip
       clip: Option<Rect>,
   }
   
   impl PaintContext {
       pub fn new() -> Self {
           Self {
               layer_builder: LayerTreeBuilder::new(),
               transform: Transform2D::identity(),
               clip: None,
           }
       }
       
       /// Push a layer
       pub fn push_layer(&mut self, layer: Layer) {
           self.layer_builder.push_layer(layer);
       }
       
       /// Pop a layer
       pub fn pop_layer(&mut self) {
           self.layer_builder.pop_layer();
       }
       
       /// Draw a rectangle
       pub fn draw_rect(&mut self, rect: Rect, paint: Paint) {
           self.layer_builder.add_primitive(Primitive::Rect {
               rect,
               paint,
           });
       }
       
       /// Draw text
       pub fn draw_text(&mut self, layout: &TextLayout, offset: Offset, color: Color) {
           self.layer_builder.add_primitive(Primitive::Text {
               layout: layout.clone(),
               offset,
               color,
           });
       }
       
       /// Paint a child at offset
       pub fn paint_child(&mut self, child: &dyn RenderObject, offset: Offset) {
           // Save transform
           let old_transform = self.transform;
           
           // Apply offset
           self.transform = self.transform.then_translate(offset.x, offset.y);
           
           // Paint child
           child.paint(self, Offset::zero());
           
           // Restore transform
           self.transform = old_transform;
       }
       
       /// Convert to scene
       pub fn into_scene(self) -> Scene {
           self.layer_builder.build()
       }
   }
   ```

**Критерії завершення**:
- [ ] PaintContext works
- [ ] Layer system works
- [ ] Transform handling works
- [ ] 30+ paint tests

---

#### День 8: Hit Testing

**Цель**: Pointer hit testing

**Задачи**:

1. **Hit Test Protocol**
   ```rust
   /// Hit test result
   pub struct HitTestResult {
       /// Path from root to hit target
       path: Vec<HitTestEntry>,
   }
   
   impl HitTestResult {
       pub fn new() -> Self {
           Self {
               path: Vec::new(),
           }
       }
       
       pub fn add(&mut self, entry: HitTestEntry) {
           self.path.push(entry);
       }
       
       pub fn path(&self) -> &[HitTestEntry] {
           &self.path
       }
   }
   
   #[derive(Clone, Debug)]
   pub struct HitTestEntry {
       pub target: RenderObjectId,
       pub local_position: Point,
       pub transform: Transform2D,
   }
   
   impl RenderPadding {
       fn hit_test_children(&self, result: &mut HitTestResult, position: Point) -> bool {
           // Transform position to child space
           let child_position = Point::new(
               position.x - self.padding.left,
               position.y - self.padding.top,
           );
           
           // Test child
           if let Some(child_id) = self.child.child_id() {
               if let Some(child) = self.child.owner().get(child_id) {
                   return child.hit_test(result, child_position);
               }
           }
           
           false
       }
   }
   
   impl RenderObject for RenderPadding {
       fn hit_test(&self, result: &mut HitTestResult, position: Point) -> bool {
           // Check if position is in bounds
           if !self.size.contains(position) {
               return false;
           }
           
           // Test children first
           if self.hit_test_children(result, position) {
               return true;
           }
           
           // Add self to result
           result.add(HitTestEntry {
               target: self.id(),
               local_position: position,
               transform: Transform2D::identity(),
           });
           
           true
       }
   }
   ```

**Критерії завершення**:
- [ ] Hit testing works
- [ ] Transform handling works
- [ ] Hit test path correct
- [ ] 35+ hit test tests

---

#### День 9: Optimization (Relayout/Repaint Boundaries)

**Цель**: Layout and paint optimization

**Задачи**:

1. **Relayout Boundary**
   ```rust
   /// Relayout boundary
   ///
   /// Isolates layout changes to this subtree.
   /// When this node is marked dirty, layout doesn't propagate up.
   pub trait RelayoutBoundary: RenderBox<impl Arity> {
       fn is_repaint_boundary(&self) -> bool {
           true
       }
   }
   
   impl RenderObject for dyn RelayoutBoundary {
       fn mark_needs_layout(&mut self) {
           // Don't propagate up - this is a boundary
           self.needs_layout = true;
           // Schedule layout with PipelineOwner
       }
   }
   ```

2. **Repaint Boundary**
   ```rust
   /// Repaint boundary
   ///
   /// Isolates paint changes to this subtree (uses separate layer).
   pub struct RenderRepaintBoundary {
       child: BoxChild<Single>,
       layer: Option<OffsetLayer>,
       needs_paint: bool,
   }
   
   impl RenderObject for RenderRepaintBoundary {
       fn paint(&self, context: &mut PaintContext, offset: Offset) {
           // Create layer for this boundary
           let mut layer_context = PaintContext::new();
           
           // Paint child to layer
           self.child.paint(&mut layer_context, Offset::zero());
           
           // Add layer to parent context
           let layer = layer_context.into_layer();
           context.push_layer(layer);
       }
   }
   ```

**Критерії завершення**:
- [ ] Relayout boundary works
- [ ] Repaint boundary works
- [ ] Performance improved
- [ ] 20+ optimization tests

---

#### День 10: Integration Testing & Documentation

**Цель**: Production readiness

**Задачи**:

1. **Integration Tests**
   ```rust
   #[test]
   fn test_full_render_tree() {
       let owner = PipelineOwner::new();
       
       // Build tree
       let text = RenderText::new("Hello".into(), TextStyle::default());
       let text_id = owner.insert(Box::new(text));
       
       let padding = RenderPadding::new(EdgeInsets::all(10.0));
       padding.set_child(text_id);
       let padding_id = owner.insert(Box::new(padding));
       
       owner.set_root(padding_id);
       
       // Layout
       owner.flush_layout();
       
       // Paint
       let scene = owner.flush_paint();
       
       // Verify
       assert!(scene.layers().len() > 0);
   }
   ```

2. **Documentation**
   - README.md
   - Architecture docs
   - API docs
   - Examples

**Критерії завершення**:
- [ ] All tests pass (200+)
- [ ] cargo doc builds
- [ ] README complete
- [ ] Examples documented

---

## Критерії Завершення Phase 6

- [ ] **flui_rendering 0.1.0**
  - [ ] RenderObject system works
  - [ ] Box protocol works
  - [ ] Flex layout works
  - [ ] Paint system works
  - [ ] Hit testing works
  - [ ] Optimizations work
  - [ ] 200+ rendering tests

---

**Статус**: 🟡 Ready for Implementation  
**Останнє оновлення**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базується на**: Flutter RenderObject + GPUI scene + original architecture design
