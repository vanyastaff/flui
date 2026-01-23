# Phase 5: Widget Tree Layer (flui-view) - Детальный План v2 (GPUI-Enhanced)

> **Базируется на**: Deep GPUI analysis + Flutter Widget/Element system  
> **Обновление**: Integrates GPUI insights (associated types, source tracking, three-phase lifecycle)  
> **Предыдущие фазы**: Phase 1-4 должны быть завершены  
> **Цель**: Production-ready View/Element architecture с GPUI-level type safety

---

## 🆕 Что Изменилось в V2

### Ключевые Улучшения из GPUI:

1. ✅ **Associated Types для Element State** - type-safe state threading
2. ✅ **Source Location Tracking** - `#[track_caller]` для debugging
3. ✅ **Three-Phase Element Lifecycle** - request_layout → prepaint → paint
4. ✅ **Inline Interactivity** - события хранятся в элементах
5. ✅ **Draw Phase Guards** - runtime safety checks

### Отличия от V1:

| Аспект | V1 (Flutter-style) | V2 (GPUI-enhanced) |
|--------|-------------------|-------------------|
| Element State | Mutable fields | Associated types + fields |
| Lifecycle Phases | mount/update/unmount | request_layout/prepaint/paint |
| Source Tracking | None | `#[track_caller]` |
| Event Listeners | Separate EventDispatcher | Inline in Element |
| Phase Safety | None | Debug assertions |

---

## Детальный План Реализации

### Этап 5.1: Enhanced Element Architecture (Неделя 9, Дни 1-3)

#### День 1: Element Trait с Associated Types

**Цель**: Define GPUI-inspired Element trait with type-safe state

**Референсы**:
- `.gpui/src/element.rs` - GPUI Element trait
- `GPUI_DEEP_ANALYSIS.md` - Associated types pattern

**Задачи**:

1. **Создать `element/trait.rs` (ENHANCED)**
   ```rust
   use std::any::Any;
   
   /// Element trait (GPUI-inspired with associated types)
   ///
   /// Elements are the mutable, stateful counterparts to Views.
   /// Unlike GPUI, we keep mount/update/unmount for compatibility with
   /// BuildOwner, but add associated types for type-safe state threading.
   ///
   /// # Three-Phase Rendering (GPUI pattern)
   ///
   /// 1. **Request Layout**: Compute layout, return LayoutState
   /// 2. **Prepaint**: Compute hitboxes, return PrepaintState  
   /// 3. **Paint**: Render using both states
   ///
   /// # State Threading
   ///
   /// Each phase returns state that's passed to subsequent phases:
   /// ```text
   /// request_layout() -> LayoutState
   ///      ↓
   /// prepaint(LayoutState) -> PrepaintState
   ///      ↓
   /// paint(LayoutState, PrepaintState)
   /// ```
   pub trait Element: 'static + Send {
       /// Layout state (computed during request_layout)
       type LayoutState: 'static;
       
       /// Prepaint state (computed during prepaint)
       type PrepaintState: 'static;
       
       /// Element ID (if has global ID)
       fn id(&self) -> Option<ElementId> {
           None
       }
       
       /// Source location (for debugging)
       ///
       /// Captured via #[track_caller] in constructors.
       #[cfg(debug_assertions)]
       fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
           None
       }
       
       // === Lifecycle (Flutter-compatible) ===
       
       /// Mount element into tree
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner);
       
       /// Update element with new view
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner);
       
       /// Unmount element from tree
       fn unmount(&mut self, owner: &mut BuildOwner);
       
       // === Three-Phase Rendering (GPUI pattern) ===
       
       /// Phase 1: Request layout
       ///
       /// Compute layout and return layout state.
       /// Called by BuildOwner during layout phase.
       fn request_layout(
           &mut self,
           id: Option<&GlobalElementId>,
           cx: &mut BuildContext,
       ) -> (LayoutId, Self::LayoutState);
       
       /// Phase 2: Prepaint
       ///
       /// Compute hitboxes and other pre-paint data.
       /// Receives LayoutState from request_layout.
       fn prepaint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &mut Self::LayoutState,
           cx: &mut BuildContext,
       ) -> Self::PrepaintState;
       
       /// Phase 3: Paint
       ///
       /// Paint using both LayoutState and PrepaintState.
       fn paint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &Self::LayoutState,
           prepaint_state: &Self::PrepaintState,
           cx: &mut PaintContext,
       );
       
       // === Helpers ===
       
       /// Visit children
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId));
       
       /// Get render object (if any)
       fn render_object(&self) -> Option<&dyn RenderObject> {
           None
       }
       
       /// Downcast to concrete type
       fn as_any(&self) -> &dyn Any;
       fn as_any_mut(&mut self) -> &mut dyn Any;
   }
   
   /// Type-erased element
   pub trait AnyElement: Element<LayoutState = Box<dyn Any>, PrepaintState = Box<dyn Any>> {
       /// Clone this element
       fn clone_element(&self) -> Box<dyn AnyElement>;
       
       /// Type name for debugging
       fn type_name(&self) -> &'static str;
   }
   
   /// Global element ID (window-scoped)
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct GlobalElementId {
       pub window_id: WindowId,
       pub element_id: ElementId,
   }
   ```

2. **Stateless Element with Associated Types**
   ```rust
   /// Stateless element (ENHANCED with associated types)
   pub struct StatelessElement<V: StatelessView> {
       /// View configuration
       view: V,
       
       /// Child element ID
       child: Option<ElementId>,
       
       /// Source location (debug only)
       #[cfg(debug_assertions)]
       source_location: Option<&'static std::panic::Location<'static>>,
   }
   
   /// Layout state for stateless element
   pub struct StatelessLayoutState {
       /// Child's layout ID
       child_layout_id: Option<LayoutId>,
   }
   
   /// Prepaint state for stateless element
   pub struct StatelessPrepaintState {
       /// Child's hitbox
       child_hitbox: Option<Hitbox>,
   }
   
   impl<V: StatelessView> Element for StatelessElement<V> {
       type LayoutState = StatelessLayoutState;
       type PrepaintState = StatelessPrepaintState;
       
       #[cfg(debug_assertions)]
       fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
           self.source_location
       }
       
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           tracing::debug!(
               "Mounting stateless element: {} at {:?}",
               std::any::type_name::<V>(),
               self.source_location()
           );
           
           // Build child
           let child_view = self.view.build(&BuildContext::new(owner));
           let child_element = child_view.create_element_any();
           let child_id = owner.insert_element(child_element);
           
           // Mount child
           owner.mount_element(child_id, Some(self.id().unwrap()), owner);
           self.child = Some(child_id);
       }
       
       fn request_layout(
           &mut self,
           id: Option<&GlobalElementId>,
           cx: &mut BuildContext,
       ) -> (LayoutId, Self::LayoutState) {
           // Request layout for child
           let child_layout_id = if let Some(child_id) = self.child {
               Some(cx.owner().request_layout_for_element(child_id, cx))
           } else {
               None
           };
           
           // Return empty layout ID (we don't have our own layout)
           let layout_id = LayoutId::default();
           let state = StatelessLayoutState { child_layout_id };
           
           (layout_id, state)
       }
       
       fn prepaint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &mut Self::LayoutState,
           cx: &mut BuildContext,
       ) -> Self::PrepaintState {
           // Prepaint child
           let child_hitbox = if let Some(child_id) = self.child {
               cx.owner().prepaint_element(child_id, bounds, cx)
           } else {
               None
           };
           
           StatelessPrepaintState { child_hitbox }
       }
       
       fn paint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &Self::LayoutState,
           prepaint_state: &Self::PrepaintState,
           cx: &mut PaintContext,
       ) {
           // Paint child
           if let Some(child_id) = self.child {
               cx.paint_element(child_id, bounds);
           }
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
           if let Some(child_id) = self.child {
               visitor(child_id);
           }
       }
       
       fn as_any(&self) -> &dyn Any {
           self
       }
       
       fn as_any_mut(&mut self) -> &mut dyn Any {
           self
       }
   }
   
   impl<V: StatelessView> StatelessElement<V> {
       /// Create new stateless element
       #[cfg(debug_assertions)]
       #[track_caller]
       pub fn new(view: V) -> Self {
           Self {
               view,
               child: None,
               source_location: Some(std::panic::Location::caller()),
           }
       }
       
       #[cfg(not(debug_assertions))]
       pub fn new(view: V) -> Self {
           Self {
               view,
               child: None,
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Element trait with associated types
- [ ] StatelessElement uses associated types
- [ ] Source location tracking works
- [ ] Three-phase lifecycle implemented
- [ ] 30+ element tests

---

#### День 2: BuildOwner с Draw Phase Tracking

**Цель**: Add GPUI-style draw phase tracking for safety

**Задачи**:

1. **Обновить `owner/build_owner.rs` (ENHANCED)**
   ```rust
   use dashmap::DashMap;
   use parking_lot::RwLock;
   use std::sync::Arc;
   
   /// Draw phase (GPUI pattern)
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum DrawPhase {
       /// Not currently drawing
       None,
       
       /// Requesting layout
       RequestLayout,
       
       /// Prepaint (computing hitboxes)
       Prepaint,
       
       /// Paint (rendering)
       Paint,
   }
   
   /// Build owner (ENHANCED with phase tracking)
   pub struct BuildOwner {
       /// Element storage
       elements: Arc<RwLock<Slab<Box<dyn AnyElement>>>>,
       
       /// Dirty elements
       dirty_elements: Arc<DashMap<ElementId, DirtyReason>>,
       
       /// Root element
       root: Arc<RwLock<Option<ElementId>>>,
       
       /// Current draw phase (GPUI pattern)
       draw_phase: Arc<RwLock<DrawPhase>>,
       
       /// Build depth (protect against infinite loops)
       build_depth: Arc<RwLock<usize>>,
       
       /// Element layout states (cached)
       layout_states: Arc<DashMap<ElementId, Box<dyn Any>>>,
       
       /// Element prepaint states (cached)
       prepaint_states: Arc<DashMap<ElementId, Box<dyn Any>>>,
   }
   
   impl BuildOwner {
       pub fn new() -> Self {
           Self {
               elements: Arc::new(RwLock::new(Slab::new())),
               dirty_elements: Arc::new(DashMap::new()),
               root: Arc::new(RwLock::new(None)),
               draw_phase: Arc::new(RwLock::new(DrawPhase::None)),
               build_depth: Arc::new(RwLock::new(0)),
               layout_states: Arc::new(DashMap::new()),
               prepaint_states: Arc::new(DashMap::new()),
           }
       }
       
       /// Get current draw phase
       pub fn draw_phase(&self) -> DrawPhase {
           *self.draw_phase.read()
       }
       
       /// Set draw phase
       fn set_draw_phase(&self, phase: DrawPhase) {
           tracing::trace!("Draw phase: {:?} -> {:?}", self.draw_phase(), phase);
           *self.draw_phase.write() = phase;
       }
       
       /// Assert we're in request_layout phase
       #[track_caller]
       pub fn assert_request_layout_phase(&self) {
           debug_assert!(
               self.draw_phase() == DrawPhase::RequestLayout,
               "Can only request layout during RequestLayout phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Assert we're in prepaint phase
       #[track_caller]
       pub fn assert_prepaint_phase(&self) {
           debug_assert!(
               matches!(self.draw_phase(), DrawPhase::Prepaint | DrawPhase::RequestLayout),
               "Can only prepaint during Prepaint phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Assert we're in paint phase
       #[track_caller]
       pub fn assert_paint_phase(&self) {
           debug_assert!(
               self.draw_phase() == DrawPhase::Paint,
               "Can only paint during Paint phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       /// Flush build (three-phase)
       pub fn flush_build(&self) {
           // Phase 1: Request Layout
           self.set_draw_phase(DrawPhase::RequestLayout);
           self.flush_request_layout();
           
           // Phase 2: Prepaint
           self.set_draw_phase(DrawPhase::Prepaint);
           self.flush_prepaint();
           
           // Phase 3: Paint
           self.set_draw_phase(DrawPhase::Paint);
           self.flush_paint();
           
           // Done
           self.set_draw_phase(DrawPhase::None);
       }
       
       fn flush_request_layout(&self) {
           let mut dirty: Vec<ElementId> = self.dirty_elements
               .iter()
               .map(|e| *e.key())
               .collect();
           
           dirty.sort_by_key(|&id| self.depth(id));
           
           for element_id in dirty {
               if let Some(mut element) = self.get_element_mut(element_id) {
                   let (layout_id, layout_state) = element.request_layout(
                       Some(&GlobalElementId {
                           window_id: WindowId::default(),
                           element_id,
                       }),
                       &mut BuildContext::new(self),
                   );
                   
                   // Cache layout state
                   self.layout_states.insert(element_id, Box::new(layout_state));
               }
           }
       }
       
       fn flush_prepaint(&self) {
           // Similar to flush_request_layout, but calls prepaint
           // TODO: Implement
       }
       
       fn flush_paint(&self) {
           // Similar, but calls paint
           // TODO: Implement
       }
   }
   ```

**Критерии завершения**:
- [ ] DrawPhase enum defined
- [ ] Phase tracking works
- [ ] Phase assertions work
- [ ] Three-phase flush implemented
- [ ] 25+ phase tracking tests

---

#### День 3: Inline Interactivity (GPUI Pattern)

**Цель**: Move event listeners into elements (GPUI style)

**Задачи**:

1. **Создать `element/interactivity.rs` (NEW)**
   ```rust
   use std::any::TypeId;
   use std::collections::HashMap;
   
   /// Element interactivity (GPUI pattern)
   ///
   /// Instead of separate EventDispatcher tree, listeners live in elements.
   /// This provides better locality and easier cleanup.
   pub struct Interactivity {
       /// Source location (debug)
       #[cfg(debug_assertions)]
       source_location: Option<&'static std::panic::Location<'static>>,
       
       /// Mouse down listeners
       mouse_down_listeners: Vec<MouseDownListener>,
       
       /// Mouse up listeners
       mouse_up_listeners: Vec<MouseUpListener>,
       
       /// Mouse move listeners
       mouse_move_listeners: Vec<MouseMoveListener>,
       
       /// Click listeners
       click_listeners: Vec<ClickListener>,
       
       /// Keyboard listeners
       key_down_listeners: Vec<KeyDownListener>,
       key_up_listeners: Vec<KeyUpListener>,
       
       /// Action listeners (type-safe commands)
       action_listeners: HashMap<TypeId, Vec<ActionListener>>,
       
       /// Tooltip
       tooltip: Option<Box<dyn AnyView>>,
       
       /// Hitbox behavior
       hitbox_behavior: HitboxBehavior,
   }
   
   type MouseDownListener = Box<dyn Fn(&MouseDownEvent, DispatchPhase, &Hitbox, &mut Window) + 'static>;
   type MouseUpListener = Box<dyn Fn(&MouseUpEvent, DispatchPhase, &Hitbox, &mut Window) + 'static>;
   type MouseMoveListener = Box<dyn Fn(&MouseMoveEvent, DispatchPhase, &Hitbox, &mut Window) + 'static>;
   type ClickListener = Box<dyn Fn(&ClickEvent, &mut Window) + 'static>;
   type KeyDownListener = Box<dyn Fn(&KeyDownEvent, DispatchPhase, &mut Window) + 'static>;
   type KeyUpListener = Box<dyn Fn(&KeyUpEvent, DispatchPhase, &mut Window) + 'static>;
   type ActionListener = Box<dyn Fn(&dyn Any, &mut Window) + 'static>;
   
   /// Hitbox behavior
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum HitboxBehavior {
       /// Block events (default for interactive elements)
       Block,
       
       /// Pass through events
       PassThrough,
   }
   
   impl Interactivity {
       /// Create new interactivity
       #[cfg(debug_assertions)]
       #[track_caller]
       pub fn new() -> Self {
           Self {
               source_location: Some(std::panic::Location::caller()),
               mouse_down_listeners: Vec::new(),
               mouse_up_listeners: Vec::new(),
               mouse_move_listeners: Vec::new(),
               click_listeners: Vec::new(),
               key_down_listeners: Vec::new(),
               key_up_listeners: Vec::new(),
               action_listeners: HashMap::new(),
               tooltip: None,
               hitbox_behavior: HitboxBehavior::Block,
           }
       }
       
       /// Add mouse down listener
       pub fn on_mouse_down<F>(&mut self, button: MouseButton, listener: F)
       where
           F: Fn(&MouseDownEvent, &mut Window) + 'static,
       {
           self.mouse_down_listeners.push(Box::new(move |event, phase, hitbox, window| {
               if phase == DispatchPhase::Bubble 
                   && event.button == button 
                   && hitbox.is_hovered(window)
               {
                   listener(event, window);
               }
           }));
       }
       
       /// Add click listener
       pub fn on_click<F>(&mut self, listener: F)
       where
           F: Fn(&ClickEvent, &mut Window) + 'static,
       {
           self.click_listeners.push(Box::new(listener));
       }
       
       /// Add action listener (type-safe)
       pub fn on_action<A: Action, F>(&mut self, listener: F)
       where
           F: Fn(&A, &mut Window) + 'static,
       {
           let type_id = TypeId::of::<A>();
           self.action_listeners
               .entry(type_id)
               .or_default()
               .push(Box::new(move |action, window| {
                   if let Some(action) = action.downcast_ref::<A>() {
                       listener(action, window);
                   }
               }));
       }
       
       /// Set tooltip
       pub fn tooltip<V: View>(&mut self, tooltip: V) {
           self.tooltip = Some(Box::new(tooltip));
       }
       
       /// Dispatch mouse down event
       pub fn dispatch_mouse_down(
           &self,
           event: &MouseDownEvent,
           phase: DispatchPhase,
           hitbox: &Hitbox,
           window: &mut Window,
       ) -> bool {
           let mut handled = false;
           
           for listener in &self.mouse_down_listeners {
               listener(event, phase, hitbox, window);
               handled = true;
           }
           
           handled
       }
       
       /// Dispatch action
       pub fn dispatch_action(
           &self,
           action: &dyn Any,
           type_id: TypeId,
           window: &mut Window,
       ) -> bool {
           if let Some(listeners) = self.action_listeners.get(&type_id) {
               for listener in listeners {
                   listener(action, window);
               }
               true
           } else {
               false
           }
       }
   }
   ```

2. **Add Interactivity to Elements**
   ```rust
   /// Interactive element wrapper
   pub struct InteractiveElement<E: Element> {
       /// Inner element
       inner: E,
       
       /// Interactivity
       interactivity: Interactivity,
       
       /// Hitbox (computed during prepaint)
       hitbox: Option<Hitbox>,
   }
   
   impl<E: Element> Element for InteractiveElement<E> {
       type LayoutState = E::LayoutState;
       type PrepaintState = (E::PrepaintState, Hitbox);
       
       fn request_layout(
           &mut self,
           id: Option<&GlobalElementId>,
           cx: &mut BuildContext,
       ) -> (LayoutId, Self::LayoutState) {
           self.inner.request_layout(id, cx)
       }
       
       fn prepaint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &mut Self::LayoutState,
           cx: &mut BuildContext,
       ) -> Self::PrepaintState {
           let inner_state = self.inner.prepaint(id, bounds, layout_state, cx);
           
           // Compute hitbox
           let hitbox = Hitbox {
               bounds,
               content_mask: ContentMask::default(),
           };
           
           self.hitbox = Some(hitbox.clone());
           
           (inner_state, hitbox)
       }
       
       fn paint(
           &mut self,
           id: Option<&GlobalElementId>,
           bounds: Bounds<Pixels>,
           layout_state: &Self::LayoutState,
           prepaint_state: &Self::PrepaintState,
           cx: &mut PaintContext,
       ) {
           let (inner_state, hitbox) = prepaint_state;
           
           // Register hitbox for event dispatch
           cx.register_hitbox(self.id().unwrap(), hitbox.clone());
           
           // Paint inner
           self.inner.paint(id, bounds, layout_state, inner_state, cx);
       }
       
       // ... other methods
   }
   ```

**Критерии завершения**:
- [ ] Interactivity struct works
- [ ] Event listeners inline in elements
- [ ] Action system works
- [ ] Hitbox registration works
- [ ] 35+ interactivity tests

---

### Этап 5.2: Advanced Element Features (Неделя 9-10, Дни 4-7)

#### День 4: Stateful Elements (Enhanced)

**Задачи**:

1. **StatefulElement with Associated Types**
   ```rust
   pub struct StatefulElement<V: StatefulView> {
       view: V,
       state: V::State,
       child: Option<ElementId>,
       
       #[cfg(debug_assertions)]
       source_location: Option<&'static std::panic::Location<'static>>,
   }
   
   /// Stateful layout state
   pub struct StatefulLayoutState<S> {
       child_layout: Option<LayoutId>,
       _state: PhantomData<S>,
   }
   
   /// Stateful prepaint state
   pub struct StatefulPrepaintState<S> {
       child_hitbox: Option<Hitbox>,
       _state: PhantomData<S>,
   }
   
   impl<V: StatefulView> Element for StatefulElement<V> {
       type LayoutState = StatefulLayoutState<V::State>;
       type PrepaintState = StatefulPrepaintState<V::State>;
       
       // Three-phase lifecycle with state access
       fn request_layout(...) -> (LayoutId, Self::LayoutState) {
           // State available during layout
           self.state.on_layout();
           // ...
       }
       
       fn prepaint(...) -> Self::PrepaintState {
           // State available during prepaint
           self.state.on_prepaint();
           // ...
       }
       
       fn paint(...) {
           // State available during paint
           self.state.on_paint();
           // ...
       }
   }
   ```

**Критерии завершения**:
- [ ] StatefulElement uses associated types
- [ ] State accessible in all phases
- [ ] Lifecycle callbacks work
- [ ] 30+ stateful tests

---

#### День 5-7: Остальные Features

Days 5-7 остаются похожими на V1, но с улучшениями:
- ViewKey reconciliation (unchanged)
- InheritedWidget (unchanged)
- GlobalKey (unchanged)
- Performance optimizations (add phase-specific optimizations)

---

## Критерии Завершения Phase 5 V2

### Обязательные Требования

- [ ] **Enhanced Element System**
  - [ ] Associated types for LayoutState & PrepaintState
  - [ ] Three-phase lifecycle (request_layout → prepaint → paint)
  - [ ] Source location tracking (#[track_caller])
  - [ ] Draw phase guards (assert_request_layout_phase, etc.)
  
- [ ] **Inline Interactivity**
  - [ ] Event listeners in elements (not separate tree)
  - [ ] Action system (type-safe commands)
  - [ ] Hitbox computation in prepaint phase
  
- [ ] **Type Safety**
  - [ ] Compile-time phase checking (via associated types)
  - [ ] Runtime phase assertions (debug mode)
  - [ ] No unsafe coercions

- [ ] **All V1 Features** + Enhancements
  - [ ] StatelessView ✅ + associated types
  - [ ] StatefulView ✅ + associated types
  - [ ] ViewKey reconciliation ✅
  - [ ] InheritedWidget ✅
  - [ ] GlobalKey ✅
  - [ ] 250+ tests (50 more than V1)

---

## Migration от V1 к V2

### Breaking Changes:

1. **Element Trait Signature**
   ```rust
   // V1:
   fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner);
   
   // V2: Same, but add:
   fn request_layout(&mut self, ...) -> (LayoutId, Self::LayoutState);
   fn prepaint(&mut self, ..., layout: &mut Self::LayoutState) -> Self::PrepaintState;
   fn paint(&mut self, ..., layout: &Self::LayoutState, prepaint: &Self::PrepaintState);
   ```

2. **BuildOwner API**
   ```rust
   // V1:
   owner.flush_build();
   
   // V2: Three separate phases
   owner.flush_request_layout();
   owner.flush_prepaint();
   owner.flush_paint();
   
   // Or combined:
   owner.flush_build(); // Calls all three
   ```

### Backward Compatibility:

- mount/update/unmount остаются для совместимости
- Можно постепенно мигрировать elements на новый API

---

## Сравнение: V1 vs V2

| Feature | V1 (Flutter) | V2 (GPUI-Enhanced) | Преимущество V2 |
|---------|-------------|-------------------|----------------|
| State Threading | Mutable fields | Associated types | Type safety |
| Phases | 2 (layout, paint) | 3 (layout, prepaint, paint) | Better hit testing |
| Source Tracking | None | #[track_caller] | Easier debugging |
| Event Listeners | Separate tree | Inline | Better locality |
| Phase Safety | None | Debug assertions | Catch errors early |
| Tests | 200+ | 250+ | Better coverage |

---

**Статус**: 🟢 Ready for Review & Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with GPUI deep analysis  
**Базируется на**: GPUI element.rs + Flutter Widget/Element + V1 plan
