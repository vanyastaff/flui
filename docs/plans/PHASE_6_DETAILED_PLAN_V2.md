# Phase 6: Render Tree Layer (flui_rendering) - Детальный План v2 (GPUI-Enhanced)

> **Базируется на**: Deep GPUI analysis + Flutter RenderObject system  
> **Обновление**: Integrates GPUI draw phase tracking + SlotMap consideration  
> **Предыдущие фазы**: Phase 1-5 V2 должны быть завершены  
> **Цель**: Production-ready RenderObject system с GPUI-level safety

---

## 🆕 Что Изменилось в V2

### Ключевые Улучшения из GPUI:

1. ✅ **Pipeline Phase Tracking** - runtime safety для layout/paint
2. ✅ **Phase Guard Assertions** - debug checks для correct API usage
3. ✅ **Hitbox System** - proper bounds + content mask
4. ⚠️ **SlotMap Consideration** - evaluate vs current Slab
5. ✅ **Source Location Tracking** - для RenderObjects

### Отличия от V1:

| Аспект | V1 (Flutter-style) | V2 (GPUI-enhanced) |
|--------|-------------------|-------------------|
| Phase Tracking | None | PipelinePhase enum |
| Safety Checks | None | #[track_caller] assertions |
| Hitbox | Simple bounds | Bounds + ContentMask |
| Storage | Slab | Slab (consider SlotMap) |
| Source Tracking | None | Optional #[track_caller] |

---

## Детальный План Реализации

### Этап 6.1: Enhanced Pipeline Architecture (Неделя 11, Дни 1-3)

#### День 1: PipelineOwner с Phase Tracking

**Цель**: Add GPUI-style pipeline phase tracking

**Задачи**:

1. **Обновить `pipeline/owner.rs` (ENHANCED)**
   ```rust
   use parking_lot::RwLock;
   use slab::Slab;
   use dashmap::DashMap;
   use std::sync::Arc;
   
   /// Pipeline phase (GPUI pattern)
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum PipelinePhase {
       /// No pipeline activity
       Idle,
       
       /// Layout phase (computing sizes)
       Layout,
       
       /// Compositing phase (building layers)
       Compositing,
       
       /// Paint phase (rendering)
       Paint,
   }
   
   /// Pipeline owner (ENHANCED with phase tracking)
   pub struct PipelineOwner {
       /// Render object storage (Slab for now, consider SlotMap)
       objects: Arc<RwLock<Slab<Box<dyn AnyRenderObject>>>>,
       
       /// Current pipeline phase
       phase: Arc<RwLock<PipelinePhase>>,
       
       /// Nodes needing layout
       nodes_needing_layout: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Nodes needing paint
       nodes_needing_paint: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Nodes needing semantics update
       nodes_needing_semantics: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Nodes needing compositing
       nodes_needing_compositing: Arc<DashMap<RenderObjectId, ()>>,
       
       /// Root render object
       root: Arc<RwLock<Option<RenderObjectId>>>,
       
       /// Hitbox registry (for hit testing)
       hitboxes: Arc<DashMap<RenderObjectId, Hitbox>>,
   }
   
   impl PipelineOwner {
       pub fn new() -> Self {
           Self {
               objects: Arc::new(RwLock::new(Slab::new())),
               phase: Arc::new(RwLock::new(PipelinePhase::Idle)),
               nodes_needing_layout: Arc::new(DashMap::new()),
               nodes_needing_paint: Arc::new(DashMap::new()),
               nodes_needing_semantics: Arc::new(DashMap::new()),
               nodes_needing_compositing: Arc::new(DashMap::new()),
               root: Arc::new(RwLock::new(None)),
               hitboxes: Arc::new(DashMap::new()),
           }
       }
       
       /// Get current pipeline phase
       pub fn phase(&self) -> PipelinePhase {
           *self.phase.read()
       }
       
       /// Set pipeline phase
       fn set_phase(&self, phase: PipelinePhase) {
           tracing::trace!("Pipeline phase: {:?} -> {:?}", self.phase(), phase);
           *self.phase.write() = phase;
       }
       
       // === Phase Assertions (GPUI pattern) ===
       
       /// Assert we're in layout phase
       #[track_caller]
       pub fn assert_layout_phase(&self) {
           debug_assert!(
               self.phase() == PipelinePhase::Layout,
               "Can only perform layout during Layout phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Assert we're in compositing phase
       #[track_caller]
       pub fn assert_compositing_phase(&self) {
           debug_assert!(
               self.phase() == PipelinePhase::Compositing,
               "Can only composite during Compositing phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Assert we're in paint phase
       #[track_caller]
       pub fn assert_paint_phase(&self) {
           debug_assert!(
               self.phase() == PipelinePhase::Paint,
               "Can only paint during Paint phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Assert we're NOT in any phase (safe to mutate tree)
       #[track_caller]
       pub fn assert_idle_phase(&self) {
           debug_assert!(
               self.phase() == PipelinePhase::Idle,
               "Can only modify tree during Idle phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       // === Pipeline Execution ===
       
       /// Flush all phases
       pub fn flush_pipeline(&self) {
           // Phase 1: Layout
           self.set_phase(PipelinePhase::Layout);
           self.flush_layout();
           
           // Phase 2: Compositing
           self.set_phase(PipelinePhase::Compositing);
           self.flush_compositing();
           
           // Phase 3: Paint
           self.set_phase(PipelinePhase::Paint);
           let scene = self.flush_paint();
           
           // Back to idle
           self.set_phase(PipelinePhase::Idle);
           
           // Return scene for rendering
           scene
       }
       
       /// Flush layout phase
       pub fn flush_layout(&self) {
           self.assert_layout_phase();
           
           tracing::debug!(
               "Flushing layout for {} nodes",
               self.nodes_needing_layout.len()
           );
           
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
                       let constraints = self.get_constraints_for_child(id);
                       object.layout(constraints);
                   }
               }
           }
       }
       
       /// Flush compositing phase
       pub fn flush_compositing(&self) {
           self.assert_compositing_phase();
           
           tracing::debug!(
               "Flushing compositing for {} nodes",
               self.nodes_needing_compositing.len()
           );
           
           // Build layer tree
           // TODO: Implement compositing
       }
       
       /// Flush paint phase
       pub fn flush_paint(&self) -> Scene {
           self.assert_paint_phase();
           
           tracing::debug!(
               "Flushing paint for {} nodes",
               self.nodes_needing_paint.len()
           );
           
           let mut context = PaintContext::new();
           
           // Paint from root
           if let Some(root_id) = *self.root.read() {
               if let Some(root) = self.get(root_id) {
                   root.paint(&mut context, Offset::zero());
               }
           }
           
           context.into_scene()
       }
       
       // === RenderObject Management ===
       
       /// Insert render object
       pub fn insert(&self, object: Box<dyn AnyRenderObject>) -> RenderObjectId {
           self.assert_idle_phase(); // Can only insert during idle
           
           let mut objects = self.objects.write();
           let index = objects.insert(object);
           RenderObjectId::new(index + 1) // +1 for NonZeroUsize
       }
       
       /// Mark node as needing layout
       pub fn mark_needs_layout(&self, id: RenderObjectId) {
           tracing::trace!("Marking {:?} as needing layout", id);
           self.nodes_needing_layout.insert(id, ());
       }
       
       /// Mark node as needing paint
       pub fn mark_needs_paint(&self, id: RenderObjectId) {
           tracing::trace!("Marking {:?} as needing paint", id);
           self.nodes_needing_paint.insert(id, ());
       }
       
       // === Hitbox Management ===
       
       /// Register hitbox for render object
       pub fn register_hitbox(&self, id: RenderObjectId, hitbox: Hitbox) {
           self.hitboxes.insert(id, hitbox);
       }
       
       /// Get hitbox for render object
       pub fn get_hitbox(&self, id: RenderObjectId) -> Option<Hitbox> {
           self.hitboxes.get(&id).map(|h| h.clone())
       }
       
       /// Hit test at position
       pub fn hit_test(&self, position: Point<f32, PhysicalPixels>) -> HitTestResult {
           let mut result = HitTestResult::new();
           
           if let Some(root_id) = *self.root.read() {
               if let Some(root) = self.get(root_id) {
                   root.hit_test(&mut result, position);
               }
           }
           
           result
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
   }
   ```

**Критерии завершения**:
- [ ] PipelinePhase enum works
- [ ] Phase tracking works
- [ ] Phase assertions work (debug mode)
- [ ] flush_pipeline executes all phases
- [ ] 30+ pipeline tests

---

#### День 2: Enhanced RenderObject Trait

**Задачи**:

1. **Обновить `traits/render_object.rs` (ENHANCED)**
   ```rust
   use ambassador::delegatable_trait;
   
   /// RenderObject trait (ENHANCED with source tracking)
   #[delegatable_trait]
   pub trait RenderObject: 'static + Send {
       /// Source location (debug only)
       #[cfg(debug_assertions)]
       fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
           None
       }
       
       /// Attach to render tree
       fn attach(&mut self, owner: PipelineOwner);
       
       /// Detach from render tree
       fn detach(&mut self);
       
       /// Mark needs layout
       fn mark_needs_layout(&mut self);
       
       /// Mark needs paint
       fn mark_needs_paint(&mut self);
       
       /// Check if needs layout
       fn needs_layout(&self) -> bool;
       
       /// Check if needs paint
       fn needs_paint(&self) -> bool;
       
       /// Perform layout
       ///
       /// SAFETY: Can only be called during Layout phase.
       /// Use PipelineOwner::assert_layout_phase() to verify.
       fn layout(&mut self, constraints: Constraints);
       
       /// Paint to context
       ///
       /// SAFETY: Can only be called during Paint phase.
       /// Use PipelineOwner::assert_paint_phase() to verify.
       fn paint(&self, context: &mut PaintContext, offset: Offset);
       
       /// Hit test
       fn hit_test(&self, result: &mut HitTestResult, position: Point) -> bool {
           false
       }
       
       /// Parent
       fn parent(&self) -> Option<RenderObjectId>;
       fn set_parent(&mut self, parent: Option<RenderObjectId>);
       
       /// Visit children
       fn visit_children(&self, visitor: &mut dyn FnMut(RenderObjectId));
       
       /// Size (if laid out)
       fn size(&self) -> Option<Size>;
       
       /// Type name
       fn type_name(&self) -> &'static str {
           std::any::type_name::<Self>()
       }
   }
   ```

2. **Hitbox System (NEW)**
   ```rust
   /// Hitbox (GPUI pattern)
   ///
   /// Combines bounds with content mask for accurate hit testing.
   #[derive(Clone, Debug)]
   pub struct Hitbox {
       /// Bounds in screen coordinates
       pub bounds: Bounds<Pixels>,
       
       /// Content mask (for clipping)
       pub content_mask: ContentMask<Pixels>,
   }
   
   impl Hitbox {
       pub fn new(bounds: Bounds<Pixels>) -> Self {
           Self {
               bounds,
               content_mask: ContentMask::default(),
           }
       }
       
       /// Check if position is inside hitbox
       pub fn contains(&self, position: Point<Pixels>) -> bool {
           self.bounds.contains(position) && self.content_mask.contains(position)
       }
       
       /// Check if hitbox is hovered (considering window state)
       pub fn is_hovered(&self, window: &Window) -> bool {
           if let Some(mouse_position) = window.mouse_position() {
               self.contains(mouse_position)
           } else {
               false
           }
       }
   }
   
   /// Content mask (for clipping)
   #[derive(Clone, Debug, Default)]
   pub struct ContentMask<T> {
       /// Clip bounds
       clip: Option<Bounds<T>>,
   }
   
   impl ContentMask<Pixels> {
       pub fn contains(&self, position: Point<Pixels>) -> bool {
           if let Some(clip) = &self.clip {
               clip.contains(position)
           } else {
               true
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] RenderObject has source location
- [ ] Hitbox system works
- [ ] ContentMask clipping works
- [ ] 25+ hitbox tests

---

#### День 3: Enhanced RenderBox Protocol

**Задачи** остаются похожими на V1, но добавляем:

1. **Source Location Tracking**
   ```rust
   impl RenderPadding {
       #[cfg(debug_assertions)]
       #[track_caller]
       pub fn new(padding: EdgeInsets) -> Self {
           Self {
               child: BoxChild::new(),
               padding,
               size: Size::zero(),
               needs_layout: true,
               needs_paint: true,
               source_location: Some(std::panic::Location::caller()),
           }
       }
   }
   ```

2. **Phase Assertions in Methods**
   ```rust
   impl RenderBox<Single> for RenderPadding {
       fn perform_layout(&mut self, constraints: BoxConstraints) {
           // Optionally assert we're in layout phase
           // (PipelineOwner will already check, but can add extra safety)
           
           // ... layout logic
       }
   }
   ```

---

### Етап 6.2: Layout Implementation (Неделя 11-12, Дні 4-6)

Days 4-6 остаются похожими на V1:
- RenderText (Leaf)
- Container RenderObjects (Padding, Center, SizedBox)  
- Flex Layout (Row, Column)

**Но добавляем в каждый**:
- Source location tracking (`#[track_caller]`)
- Hitbox registration в paint methods

---

### Етап 6.3: Paint & Hit Testing (Неделя 12, Дні 7-10)

#### День 7: Enhanced Paint Context

**Задачи**:

1. **PaintContext with Hitbox Registration**
   ```rust
   /// Paint context (ENHANCED)
   pub struct PaintContext {
       /// Layer tree builder
       layer_builder: LayerTreeBuilder,
       
       /// Current transform
       transform: Transform2D,
       
       /// Current clip
       clip: Option<Rect>,
       
       /// Hitbox registry (NEW)
       hitbox_registry: Arc<DashMap<RenderObjectId, Hitbox>>,
       
       /// Current pipeline owner (for registration)
       owner: Weak<PipelineOwner>,
   }
   
   impl PaintContext {
       /// Register hitbox for render object
       pub fn register_hitbox(&mut self, id: RenderObjectId, bounds: Bounds<Pixels>) {
           let hitbox = Hitbox {
               bounds,
               content_mask: self.current_content_mask(),
           };
           
           if let Some(owner) = self.owner.upgrade() {
               owner.register_hitbox(id, hitbox.clone());
           }
           
           self.hitbox_registry.insert(id, hitbox);
       }
       
       fn current_content_mask(&self) -> ContentMask<Pixels> {
           ContentMask {
               clip: self.clip,
           }
       }
   }
   ```

**Критерії завершення**:
- [ ] Hitbox registration works
- [ ] Content mask computed correctly
- [ ] 30+ paint context tests

---

#### День 8-10: Hit Testing & Optimization

Days 8-10 остаются похожими на V1:
- Hit testing implementation
- Relayout/Repaint boundaries
- Integration testing

**Но добавляем**:
- Hitbox-based hit testing (more accurate)
- Phase-specific optimizations

---

## Критерії Завершення Phase 6 V2

### Обязательные Требования

- [ ] **Enhanced Pipeline System**
  - [ ] Pipeline phase tracking (Idle/Layout/Compositing/Paint)
  - [ ] Phase assertions (#[track_caller])
  - [ ] Three-phase flush (layout → compositing → paint)
  
- [ ] **Hitbox System**
  - [ ] Hitbox registration during paint
  - [ ] ContentMask for clipping
  - [ ] Accurate hit testing
  
- [ ] **Source Location Tracking**
  - [ ] RenderObjects track creation location (debug mode)
  - [ ] Better error messages
  
- [ ] **All V1 Features** + Enhancements
  - [ ] Box protocol ✅
  - [ ] Flex layout ✅
  - [ ] Paint system ✅ + hitboxes
  - [ ] Hit testing ✅ + content mask
  - [ ] 220+ tests (20 more than V1)

---

## SlotMap Consideration

### Current: Slab

**Pros**:
- Simple, well-tested
- Stable IDs via NonZeroUsize
- O(1) access

**Cons**:
- No generation tracking
- Can't detect dangling references
- Manual index management (+1/-1 for NonZeroUsize)

### Alternative: SlotMap

**Pros**:
- Automatic generation tracking
- Detects dangling references
- Cleaner API (no +1/-1)

**Cons**:
- Slightly more overhead
- Less familiar to team

### Recommendation

**Для Phase 6**: Stick with Slab
- Less риска при первой реализации
- Можем мигрировать на SlotMap позже
- Performance difference minimal

**Для будущего**: Consider SlotMap
- Добавить в Phase 6.5 или Phase 7
- Create migration guide
- A/B test performance

---

## Сравнение: V1 vs V2

| Feature | V1 (Flutter) | V2 (GPUI-Enhanced) | Преимущество V2 |
|---------|-------------|-------------------|----------------|
| Phase Tracking | None | PipelinePhase enum | Safety |
| Phase Assertions | None | #[track_caller] | Catch errors |
| Hitbox | Simple Bounds | Bounds + ContentMask | Accuracy |
| Source Tracking | None | Optional | Debug |
| Tests | 200+ | 220+ | Better coverage |

---

**Статус**: 🟢 Ready for Review & Implementation  
**Останнє оновлення**: 2026-01-22  
**Автор**: Claude with GPUI deep analysis  
**Базується на**: GPUI window.rs + Flutter RenderObject + V1 plan
