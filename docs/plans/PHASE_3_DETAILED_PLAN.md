# Phase 3: Interaction Layer - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущие фазы**: Phase 1 (Foundation) + Phase 2 (Rendering) должны быть завершены  
> **Референсы**: `.gpui/src/interactive.rs`, `.gpui/src/input.rs`, Flutter gesture system  
> **Цель**: Production-ready event routing с hit testing, focus management и gesture recognition

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui_interaction
- ✅ Cargo.toml с зависимостями (ui-events, cursor-icon, parking_lot, dashmap)
- ✅ Модульная структура: `routing/`, `recognizers/`, `processing/`, `testing/`
- ✅ Базовые файлы: `events.rs`, `ids.rs`, `traits.rs`, `arena.rs`, `mouse_tracker.rs`
- ✅ Type-safe IDs система
- ✅ Sealed traits infrastructure
- ✅ Focus infrastructure начато

#### flui_types
- ✅ Geometry types для hit testing (Rect, Point, Offset)
- ✅ Event types в `gestures/` модуле

### ❌ Что Нужно Доделать / Улучшить

#### Event Routing
1. **EventRouter** - центральный dispatcher для всех событий
2. **Hit Testing** - spatial queries для определения target элементов
3. **Event Bubbling** - capture → target → bubble фазы
4. **Pointer Capture** - захват pointer events для drag operations

#### Focus Management
1. **FocusManager** - global singleton для keyboard focus
2. **FocusScope** - группировка focusable elements
3. **Focus Traversal** - Tab/Shift+Tab navigation
4. **Focus Events** - onFocus, onBlur callbacks

#### Gesture Recognition
1. **GestureArena** - conflict resolution между recognizers
2. **Tap Recognizer** - single/double/long tap
3. **Drag Recognizer** - панорамирование
4. **Scale Recognizer** - pinch-to-zoom
5. **Pan Recognizer** - swipe gestures
6. **Custom Recognizers** - extensibility

---

## Детальный План Реализации

### Этап 3.1: Event Routing & Hit Testing (Неделя 5, Дни 1-3)

#### День 1: Core Event Types & Routing Infrastructure

**Цель**: Определить event types и создать EventRouter

**Референсы**:
- `.gpui/src/interactive.rs` - GPUI event types
- `.gpui/src/platform.rs` - Platform event handling
- `ui-events` crate - W3C-compliant events

**Задачи**:

1. **Финализировать `events.rs`**
   ```rust
   use flui_types::{Point, Offset, PhysicalPixels};
   use ui_events::*;
   
   /// Platform-agnostic event wrapper
   #[derive(Clone, Debug)]
   pub enum Event {
       Pointer(PointerEvent),
       Keyboard(KeyboardEvent),
       Focus(FocusEvent),
       Lifecycle(LifecycleEvent),
   }
   
   /// Pointer event (mouse/touch/stylus)
   #[derive(Clone, Debug)]
   pub enum PointerEvent {
       Down {
           pointer_id: PointerId,
           position: Point<f32, PhysicalPixels>,
           button: MouseButton,
           modifiers: Modifiers,
           click_count: u32,
       },
       Up {
           pointer_id: PointerId,
           position: Point<f32, PhysicalPixels>,
           button: MouseButton,
           modifiers: Modifiers,
       },
       Move {
           pointer_id: PointerId,
           position: Point<f32, PhysicalPixels>,
           modifiers: Modifiers,
       },
       Enter {
           pointer_id: PointerId,
           position: Point<f32, PhysicalPixels>,
       },
       Leave {
           pointer_id: PointerId,
       },
       Scroll {
           delta: Offset<f32, PhysicalPixels>,
           position: Point<f32, PhysicalPixels>,
           modifiers: Modifiers,
       },
   }
   
   /// Keyboard event
   #[derive(Clone, Debug)]
   pub enum KeyboardEvent {
       Down {
           key: Key,
           code: KeyCode,
           modifiers: Modifiers,
           is_repeat: bool,
       },
       Up {
           key: Key,
           code: KeyCode,
           modifiers: Modifiers,
       },
       Character {
           character: String,
           modifiers: Modifiers,
       },
   }
   
   /// Focus event
   #[derive(Clone, Debug)]
   pub enum FocusEvent {
       FocusIn { focus_id: FocusNodeId },
       FocusOut { focus_id: FocusNodeId },
   }
   
   /// Modifier keys state
   #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
   pub struct Modifiers {
       pub shift: bool,
       pub ctrl: bool,
       pub alt: bool,
       pub meta: bool,
   }
   
   /// Mouse button
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum MouseButton {
       Left,
       Right,
       Middle,
       Other(u16),
   }
   ```

2. **Создать `routing/event_router.rs`**
   ```rust
   use dashmap::DashMap;
   use std::sync::Arc;
   
   /// Central event router
   ///
   /// Routes events to appropriate handlers based on hit testing results.
   /// Thread-safe singleton.
   pub struct EventRouter {
       /// Registered event handlers
       handlers: Arc<DashMap<HandlerId, EventHandler>>,
       
       /// Pointer state tracker
       pointer_state: Arc<PointerStateTracker>,
       
       /// Focus manager reference
       focus_manager: Arc<FocusManager>,
       
       /// Gesture arena for recognizer conflict resolution
       gesture_arena: Arc<Mutex<GestureArena>>,
   }
   
   impl EventRouter {
       pub fn new(focus_manager: Arc<FocusManager>) -> Self {
           Self {
               handlers: Arc::new(DashMap::new()),
               pointer_state: Arc::new(PointerStateTracker::new()),
               focus_manager,
               gesture_arena: Arc::new(Mutex::new(GestureArena::new())),
           }
       }
       
       /// Register an event handler
       pub fn register_handler(&self, handler: EventHandler) -> HandlerId {
           let id = HandlerId::new();
           self.handlers.insert(id, handler);
           id
       }
       
       /// Unregister an event handler
       pub fn unregister_handler(&self, id: HandlerId) {
           self.handlers.remove(&id);
       }
       
       /// Route an event to appropriate handlers
       pub fn route_event<H: Hittable>(
           &self,
           event: &Event,
           root: &H,
       ) -> EventResult {
           match event {
               Event::Pointer(pointer_event) => {
                   self.route_pointer_event(pointer_event, root)
               }
               Event::Keyboard(keyboard_event) => {
                   self.route_keyboard_event(keyboard_event)
               }
               Event::Focus(focus_event) => {
                   self.route_focus_event(focus_event)
               }
               Event::Lifecycle(lifecycle_event) => {
                   EventResult::Unhandled
               }
           }
       }
       
       fn route_pointer_event<H: Hittable>(
           &self,
           event: &PointerEvent,
           root: &H,
       ) -> EventResult {
           // Update pointer state
           let position = event.position();
           self.pointer_state.update(event);
           
           // Perform hit testing
           let hit_results = self.hit_test(position, root);
           
           // Feed to gesture arena
           let gesture_events = self.gesture_arena.lock()
               .process_pointer_event(event, &hit_results);
           
           // Route through event phases
           self.route_with_phases(&hit_results, event)
       }
       
       fn route_keyboard_event(&self, event: &KeyboardEvent) -> EventResult {
           // Route to focused element
           if let Some(focus_id) = self.focus_manager.focused_node() {
               if let Some(handler) = self.handlers.get(&focus_id.into()) {
                   return handler.handle_keyboard(event);
               }
           }
           
           EventResult::Unhandled
       }
   }
   
   /// Event handler ID (type-safe)
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct HandlerId(u64);
   
   impl HandlerId {
       fn new() -> Self {
           use std::sync::atomic::{AtomicU64, Ordering};
           static NEXT_ID: AtomicU64 = AtomicU64::new(1);
           Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
       }
   }
   
   /// Event handler callback wrapper
   pub struct EventHandler {
       pointer_handler: Option<Box<dyn Fn(&PointerEvent) -> EventResult + Send + Sync>>,
       keyboard_handler: Option<Box<dyn Fn(&KeyboardEvent) -> EventResult + Send + Sync>>,
   }
   
   impl EventHandler {
       pub fn builder() -> EventHandlerBuilder {
           EventHandlerBuilder::default()
       }
       
       pub fn handle_pointer(&self, event: &PointerEvent) -> EventResult {
           if let Some(handler) = &self.pointer_handler {
               handler(event)
           } else {
               EventResult::Unhandled
           }
       }
       
       pub fn handle_keyboard(&self, event: &KeyboardEvent) -> EventResult {
           if let Some(handler) = &self.keyboard_handler {
               handler(event)
           } else {
               EventResult::Unhandled
           }
       }
   }
   
   #[derive(Default)]
   pub struct EventHandlerBuilder {
       pointer_handler: Option<Box<dyn Fn(&PointerEvent) -> EventResult + Send + Sync>>,
       keyboard_handler: Option<Box<dyn Fn(&KeyboardEvent) -> EventResult + Send + Sync>>,
   }
   
   impl EventHandlerBuilder {
       pub fn on_pointer<F>(mut self, handler: F) -> Self
       where
           F: Fn(&PointerEvent) -> EventResult + Send + Sync + 'static,
       {
           self.pointer_handler = Some(Box::new(handler));
           self
       }
       
       pub fn on_keyboard<F>(mut self, handler: F) -> Self
       where
           F: Fn(&KeyboardEvent) -> EventResult + Send + Sync + 'static,
       {
           self.keyboard_handler = Some(Box::new(handler));
           self
       }
       
       pub fn build(self) -> EventHandler {
           EventHandler {
               pointer_handler: self.pointer_handler,
               keyboard_handler: self.keyboard_handler,
           }
       }
   }
   
   /// Event routing result
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum EventResult {
       /// Event was handled, stop propagation
       Handled,
       /// Event was not handled, continue propagation
       Unhandled,
   }
   
   impl PointerEvent {
       fn position(&self) -> Point<f32, PhysicalPixels> {
           match self {
               PointerEvent::Down { position, .. } => *position,
               PointerEvent::Up { position, .. } => *position,
               PointerEvent::Move { position, .. } => *position,
               PointerEvent::Enter { position, .. } => *position,
               PointerEvent::Leave { .. } => Point::zero(),
               PointerEvent::Scroll { position, .. } => *position,
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Event types comprehensive
- [ ] EventRouter routes to handlers
- [ ] Handler registration/unregistration works
- [ ] 30+ event routing tests

---

#### День 2: Hit Testing System

**Цель**: Spatial queries для определения event targets

**Референсы**:
- Plan `3.4.3 Hit Testing` - алгоритм hit testing
- Flutter's hit testing system

**Задачи**:

1. **Создать `routing/hit_test.rs`**
   ```rust
   use flui_types::{Point, Rect, PhysicalPixels};
   
   /// Trait for objects that can be hit-tested
   pub trait Hittable: Send + Sync {
       /// Test if this object is hit by the given point
       ///
       /// Returns true if hit, and adds entries to result.
       fn hit_test(
           &self,
           position: Point<f32, PhysicalPixels>,
           result: &mut HitTestResult,
       ) -> bool;
       
       /// Get the bounds of this object for optimization
       fn bounds(&self) -> Rect<f32, PhysicalPixels>;
       
       /// Get the handler ID for this object
       fn handler_id(&self) -> Option<HandlerId>;
   }
   
   /// Hit test result (list of hit targets from deepest to shallowest)
   #[derive(Default)]
   pub struct HitTestResult {
       targets: Vec<HitTestEntry>,
   }
   
   impl HitTestResult {
       pub fn new() -> Self {
           Self::default()
       }
       
       pub fn add(&mut self, entry: HitTestEntry) {
           self.targets.push(entry);
       }
       
       pub fn targets(&self) -> &[HitTestEntry] {
           &self.targets
       }
       
       pub fn is_empty(&self) -> bool {
           self.targets.is_empty()
       }
       
       /// Clear for reuse
       pub fn clear(&mut self) {
           self.targets.clear();
       }
   }
   
   /// Single hit test entry
   #[derive(Clone, Debug)]
   pub struct HitTestEntry {
       /// Handler ID for this target
       pub handler_id: HandlerId,
       
       /// Local position within this target
       pub local_position: Point<f32, PhysicalPixels>,
       
       /// Transform from root to this target
       pub transform: Matrix4,
       
       /// Depth in the tree (for sorting)
       pub depth: usize,
   }
   
   /// Hit test implementation for common shapes
   impl Hittable for Rect<f32, PhysicalPixels> {
       fn hit_test(
           &self,
           position: Point<f32, PhysicalPixels>,
           result: &mut HitTestResult,
       ) -> bool {
           if self.contains(position) {
               result.add(HitTestEntry {
                   handler_id: HandlerId::new(), // Placeholder
                   local_position: position - self.origin.to_vector(),
                   transform: Matrix4::identity(),
                   depth: 0,
                   });
               true
           } else {
               false
           }
       }
       
       fn bounds(&self) -> Rect<f32, PhysicalPixels> {
           *self
       }
       
       fn handler_id(&self) -> Option<HandlerId> {
           None
       }
   }
   ```

2. **Event Phase Routing**
   ```rust
   impl EventRouter {
       /// Route event through capture → target → bubble phases
       fn route_with_phases(
           &self,
           hit_results: &HitTestResult,
           event: &PointerEvent,
       ) -> EventResult {
           // Capture phase (root → target)
           for entry in hit_results.targets().iter().rev() {
               if let Some(handler) = self.handlers.get(&entry.handler_id) {
                   let result = handler.handle_pointer(event);
                   if result == EventResult::Handled {
                       return EventResult::Handled;
                   }
               }
           }
           
           // Target phase
           if let Some(entry) = hit_results.targets().first() {
               if let Some(handler) = self.handlers.get(&entry.handler_id) {
                   let result = handler.handle_pointer(event);
                   if result == EventResult::Handled {
                       return EventResult::Handled;
                   }
               }
           }
           
           // Bubble phase (target → root)
           for entry in hit_results.targets() {
               if let Some(handler) = self.handlers.get(&entry.handler_id) {
                   let result = handler.handle_pointer(event);
                   if result == EventResult::Handled {
                       return EventResult::Handled;
                   }
               }
           }
           
           EventResult::Unhandled
       }
       
       /// Perform hit test
       fn hit_test<H: Hittable>(
           &self,
           position: Point<f32, PhysicalPixels>,
           root: &H,
       ) -> HitTestResult {
           let mut result = HitTestResult::new();
           root.hit_test(position, &mut result);
           result
       }
   }
   ```

**Критерии завершения**:
- [ ] Hit testing accurate
- [ ] Event phases work correctly
- [ ] Transform handling correct
- [ ] 25+ hit testing tests

---

#### День 3: Pointer State & Capture

**Цель**: Track pointer state и implement pointer capture

**Задачи**:

1. **Pointer State Tracker (обновить `mouse_tracker.rs`)**
   ```rust
   use std::collections::HashMap;
   
   /// Tracks state of all active pointers
   pub struct PointerStateTracker {
       pointers: HashMap<PointerId, PointerState>,
   }
   
   #[derive(Clone, Debug)]
   pub struct PointerState {
       pub position: Point<f32, PhysicalPixels>,
       pub buttons: ButtonState,
       pub modifiers: Modifiers,
       pub captured_by: Option<HandlerId>,
   }
   
   #[derive(Copy, Clone, Debug, Default)]
   pub struct ButtonState {
       pub left: bool,
       pub right: bool,
       pub middle: bool,
   }
   
   impl PointerStateTracker {
       pub fn new() -> Self {
           Self {
               pointers: HashMap::new(),
           }
       }
       
       pub fn update(&mut self, event: &PointerEvent) {
           let pointer_id = event.pointer_id();
           
           match event {
               PointerEvent::Down { position, button, modifiers, .. } => {
                   let state = self.pointers.entry(pointer_id).or_insert_with(|| {
                       PointerState {
                           position: *position,
                           buttons: ButtonState::default(),
                           modifiers: *modifiers,
                           captured_by: None,
                       }
                   });
                   
                   state.position = *position;
                   state.modifiers = *modifiers;
                   
                   match button {
                       MouseButton::Left => state.buttons.left = true,
                       MouseButton::Right => state.buttons.right = true,
                       MouseButton::Middle => state.buttons.middle = true,
                       _ => {}
                   }
               }
               
               PointerEvent::Up { position, button, .. } => {
                   if let Some(state) = self.pointers.get_mut(&pointer_id) {
                       state.position = *position;
                       
                       match button {
                           MouseButton::Left => state.buttons.left = false,
                           MouseButton::Right => state.buttons.right = false,
                           MouseButton::Middle => state.buttons.middle = false,
                           _ => {}
                       }
                       
                       // If no buttons down, remove pointer
                       if !state.buttons.left && !state.buttons.right && !state.buttons.middle {
                           self.pointers.remove(&pointer_id);
                       }
                   }
               }
               
               PointerEvent::Move { position, modifiers, .. } => {
                   if let Some(state) = self.pointers.get_mut(&pointer_id) {
                       state.position = *position;
                       state.modifiers = *modifiers;
                   }
               }
               
               _ => {}
           }
       }
       
       pub fn get_state(&self, pointer_id: PointerId) -> Option<&PointerState> {
           self.pointers.get(&pointer_id)
       }
       
       pub fn capture(&mut self, pointer_id: PointerId, handler_id: HandlerId) {
           if let Some(state) = self.pointers.get_mut(&pointer_id) {
               state.captured_by = Some(handler_id);
           }
       }
       
       pub fn release_capture(&mut self, pointer_id: PointerId) {
           if let Some(state) = self.pointers.get_mut(&pointer_id) {
               state.captured_by = None;
           }
       }
   }
   
   impl PointerEvent {
       fn pointer_id(&self) -> PointerId {
           match self {
               PointerEvent::Down { pointer_id, .. } => *pointer_id,
               PointerEvent::Up { pointer_id, .. } => *pointer_id,
               PointerEvent::Move { pointer_id, .. } => *pointer_id,
               PointerEvent::Enter { pointer_id, .. } => *pointer_id,
               PointerEvent::Leave { pointer_id } => *pointer_id,
               PointerEvent::Scroll { .. } => PointerId(0), // Mouse wheel
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Pointer state tracked correctly
- [ ] Capture/release works
- [ ] Multi-pointer support
- [ ] 20+ pointer state tests

---

### Этап 3.2: Focus Management (Неделя 5-6, Дни 4-6)

#### День 4: Focus Manager

**Цель**: Global keyboard focus management

**Референсы**:
- Plan `3.4 flui_interaction` - Focus management spec

**Задачи**:

1. **Создать `routing/focus_manager.rs`**
   ```rust
   use once_cell::sync::Lazy;
   use parking_lot::RwLock;
   
   /// Global focus manager (singleton)
   ///
   /// Manages keyboard focus for the entire application.
   /// Thread-safe via RwLock.
   pub struct FocusManager {
       state: RwLock<FocusState>,
   }
   
   struct FocusState {
       /// Currently focused node
       focused: Option<FocusNodeId>,
       
       /// Focus history (for restoring focus)
       history: Vec<FocusNodeId>,
       
       /// Registered focus nodes
       nodes: HashMap<FocusNodeId, FocusNode>,
       
       /// Focus scopes
       scopes: HashMap<FocusScopeId, FocusScope>,
   }
   
   impl FocusManager {
       /// Get global instance
       pub fn global() -> &'static Self {
           static INSTANCE: Lazy<FocusManager> = Lazy::new(|| {
               FocusManager {
                   state: RwLock::new(FocusState {
                       focused: None,
                       history: Vec::new(),
                       nodes: HashMap::new(),
                       scopes: HashMap::new(),
                   }),
               }
           });
           &INSTANCE
       }
       
       /// Request focus for a node
       pub fn request_focus(&self, node_id: FocusNodeId) -> bool {
           let mut state = self.state.write();
           
           // Check if node is focusable
           if let Some(node) = state.nodes.get(&node_id) {
               if !node.focusable {
                   return false;
               }
               
               // Unfocus current
               if let Some(old_focus) = state.focused {
                   self.trigger_blur(old_focus, &state);
               }
               
               // Focus new
               state.focused = Some(node_id);
               state.history.push(node_id);
               self.trigger_focus(node_id, &state);
               
               true
           } else {
               false
           }
       }
       
       /// Unfocus current node
       pub fn unfocus(&self) {
           let mut state = self.state.write();
           
           if let Some(focus_id) = state.focused.take() {
               self.trigger_blur(focus_id, &state);
           }
       }
       
       /// Check if node has focus
       pub fn has_focus(&self, node_id: FocusNodeId) -> bool {
           self.state.read().focused == Some(node_id)
       }
       
       /// Get currently focused node
       pub fn focused_node(&self) -> Option<FocusNodeId> {
           self.state.read().focused
       }
       
       /// Register a focus node
       pub fn register_node(&self, node: FocusNode) -> FocusNodeId {
           let mut state = self.state.write();
           let id = FocusNodeId::new();
           state.nodes.insert(id, node);
           id
       }
       
       /// Unregister a focus node
       pub fn unregister_node(&self, node_id: FocusNodeId) {
           let mut state = self.state.write();
           
           // If this node has focus, unfocus it
           if state.focused == Some(node_id) {
               state.focused = None;
           }
           
           state.nodes.remove(&node_id);
           state.history.retain(|&id| id != node_id);
       }
       
       fn trigger_focus(&self, node_id: FocusNodeId, state: &FocusState) {
           if let Some(node) = state.nodes.get(&node_id) {
               if let Some(callback) = &node.on_focus {
                   callback(FocusEvent::FocusIn { focus_id: node_id });
               }
           }
       }
       
       fn trigger_blur(&self, node_id: FocusNodeId, state: &FocusState) {
           if let Some(node) = state.nodes.get(&node_id) {
               if let Some(callback) = &node.on_blur {
                   callback(FocusEvent::FocusOut { focus_id: node_id });
               }
           }
       }
   }
   
   /// Focus node registration
   pub struct FocusNode {
       pub focusable: bool,
       pub on_focus: Option<Box<dyn Fn(FocusEvent) + Send + Sync>>,
       pub on_blur: Option<Box<dyn Fn(FocusEvent) + Send + Sync>>,
       pub scope_id: Option<FocusScopeId>,
   }
   
   impl FocusNode {
       pub fn builder() -> FocusNodeBuilder {
           FocusNodeBuilder::default()
       }
   }
   
   #[derive(Default)]
   pub struct FocusNodeBuilder {
       focusable: bool,
       on_focus: Option<Box<dyn Fn(FocusEvent) + Send + Sync>>,
       on_blur: Option<Box<dyn Fn(FocusEvent) + Send + Sync>>,
       scope_id: Option<FocusScopeId>,
   }
   
   impl FocusNodeBuilder {
       pub fn focusable(mut self, focusable: bool) -> Self {
           self.focusable = focusable;
           self
       }
       
       pub fn on_focus<F>(mut self, callback: F) -> Self
       where
           F: Fn(FocusEvent) + Send + Sync + 'static,
       {
           self.on_focus = Some(Box::new(callback));
           self
       }
       
       pub fn on_blur<F>(mut self, callback: F) -> Self
       where
           F: Fn(FocusEvent) + Send + Sync + 'static,
       {
           self.on_blur = Some(Box::new(callback));
           self
       }
       
       pub fn scope(mut self, scope_id: FocusScopeId) -> Self {
           self.scope_id = Some(scope_id);
           self
       }
       
       pub fn build(self) -> FocusNode {
           FocusNode {
               focusable: self.focusable,
               on_focus: self.on_focus,
               on_blur: self.on_blur,
               scope_id: self.scope_id,
           }
       }
   }
   
   /// Type-safe focus node ID
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct FocusNodeId(u64);
   
   impl FocusNodeId {
       fn new() -> Self {
           use std::sync::atomic::{AtomicU64, Ordering};
           static NEXT_ID: AtomicU64 = AtomicU64::new(1);
           Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
       }
   }
   ```

**Критерии завершения**:
- [ ] Focus manager singleton works
- [ ] Focus/unfocus operations correct
- [ ] Focus callbacks triggered
- [ ] 25+ focus manager tests

---

#### День 5: Focus Scopes & Traversal

**Цель**: Tab navigation и focus scopes

**Задачи**:

1. **Focus Scope**
   ```rust
   /// Focus scope (groups focusable elements)
   pub struct FocusScope {
       pub id: FocusScopeId,
       pub nodes: Vec<FocusNodeId>,
       pub traversal_policy: FocusTraversalPolicy,
   }
   
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct FocusScopeId(u64);
   
   /// Focus traversal policy
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum FocusTraversalPolicy {
       /// Linear order (insertion order)
       Linear,
       /// Spatial (based on position)
       Spatial,
       /// Custom order
       Custom,
   }
   
   impl FocusManager {
       /// Move focus to next node in scope (Tab)
       pub fn focus_next(&self) -> bool {
           let state = self.state.read();
           
           let current = state.focused?;
           let node = state.nodes.get(&current)?;
           let scope_id = node.scope_id?;
           let scope = state.scopes.get(&scope_id)?;
           
           // Find current index
           let current_idx = scope.nodes.iter().position(|&id| id == current)?;
           
           // Next node (wrap around)
           let next_idx = (current_idx + 1) % scope.nodes.len();
           let next_id = scope.nodes[next_idx];
           
           drop(state);
           self.request_focus(next_id)
       }
       
       /// Move focus to previous node (Shift+Tab)
       pub fn focus_previous(&self) -> bool {
           let state = self.state.read();
           
           let current = state.focused?;
           let node = state.nodes.get(&current)?;
           let scope_id = node.scope_id?;
           let scope = state.scopes.get(&scope_id)?;
           
           let current_idx = scope.nodes.iter().position(|&id| id == current)?;
           
           let prev_idx = if current_idx == 0 {
               scope.nodes.len() - 1
           } else {
               current_idx - 1
           };
           
           let prev_id = scope.nodes[prev_idx];
           
           drop(state);
           self.request_focus(prev_id)
       }
       
       /// Register a focus scope
       pub fn register_scope(&self, scope: FocusScope) -> FocusScopeId {
           let mut state = self.state.write();
           let id = scope.id;
           state.scopes.insert(id, scope);
           id
       }
   }
   ```

**Критерии завершения**:
- [ ] Tab navigation works
- [ ] Shift+Tab works
- [ ] Scope containment correct
- [ ] 20+ traversal tests

---

#### День 6: Keyboard Event Integration

**Цель**: Integrate keyboard events с focus system

**Задачи**:

1. **Keyboard Event Routing**
   ```rust
   impl EventRouter {
       fn route_keyboard_event(&self, event: &KeyboardEvent) -> EventResult {
           // Check for focus traversal keys
           match event {
               KeyboardEvent::Down { key, code, modifiers, .. } => {
                   // Tab navigation
                   if *code == KeyCode::Tab {
                       if modifiers.shift {
                           self.focus_manager.focus_previous();
                       } else {
                           self.focus_manager.focus_next();
                       }
                       return EventResult::Handled;
                   }
               }
               _ => {}
           }
           
           // Route to focused element
           if let Some(focus_id) = self.focus_manager.focused_node() {
               if let Some(handler) = self.handlers.get(&focus_id.into()) {
                   return handler.handle_keyboard(event);
               }
           }
           
           EventResult::Unhandled
       }
   }
   ```

**Критерии завершения**:
- [ ] Keyboard events route to focused element
- [ ] Tab handling works
- [ ] Keyboard shortcuts work
- [ ] 15+ keyboard integration tests

---

### Этап 3.3: Gesture Recognition (Неделя 6, Дни 7-10)

#### День 7: Gesture Arena

**Цель**: Conflict resolution framework

**Референсы**:
- Plan `3.4.4 Gesture Recognition` - Gesture Arena spec
- Flutter gesture arena

**Задачи**:

1. **Финализировать `arena.rs`**
   ```rust
   /// Gesture arena (resolves conflicts between recognizers)
   pub struct GestureArena {
       recognizers: Vec<Box<dyn GestureRecognizer>>,
       active_gestures: HashMap<PointerId, ArenaState>,
   }
   
   struct ArenaState {
       competing: Vec<RecognizerId>,
       winner: Option<RecognizerId>,
   }
   
   impl GestureArena {
       pub fn new() -> Self {
           Self {
               recognizers: Vec::new(),
               active_gestures: HashMap::new(),
           }
       }
       
       pub fn add_recognizer(&mut self, recognizer: Box<dyn GestureRecognizer>) {
           self.recognizers.push(recognizer);
       }
       
       pub fn process_pointer_event(
           &mut self,
           event: &PointerEvent,
           hit_results: &HitTestResult,
       ) -> Vec<GestureEvent> {
           let mut gesture_events = Vec::new();
           let pointer_id = event.pointer_id();
           
           // Feed to all recognizers
           for recognizer in &mut self.recognizers {
               if let Some(result) = recognizer.process_event(event, hit_results) {
                   match result {
                       RecognizerResult::Possible => {
                           // Add to arena
                           self.active_gestures.entry(pointer_id)
                               .or_insert_with(|| ArenaState {
                                   competing: Vec::new(),
                                   winner: None,
                               })
                               .competing.push(recognizer.id());
                       }
                       
                       RecognizerResult::Recognized(gesture) => {
                           // This recognizer wins!
                           gesture_events.push(gesture);
                           self.declare_winner(pointer_id, recognizer.id());
                       }
                       
                       RecognizerResult::Failed => {
                           // Remove from arena
                           if let Some(state) = self.active_gestures.get_mut(&pointer_id) {
                               state.competing.retain(|&id| id != recognizer.id());
                           }
                       }
                   }
               }
           }
           
           gesture_events
       }
       
       fn declare_winner(&mut self, pointer_id: PointerId, winner_id: RecognizerId) {
           if let Some(state) = self.active_gestures.get_mut(&pointer_id) {
               state.winner = Some(winner_id);
               
               // Reject all other recognizers
               for recognizer in &mut self.recognizers {
                   if recognizer.id() != winner_id {
                       recognizer.reject();
                   }
               }
           }
       }
   }
   
   /// Gesture recognizer trait
   pub trait GestureRecognizer: Send + Sync {
       fn id(&self) -> RecognizerId;
       
       fn process_event(
           &mut self,
           event: &PointerEvent,
           hit_results: &HitTestResult,
       ) -> Option<RecognizerResult>;
       
       fn reject(&mut self);
   }
   
   /// Recognizer result
   pub enum RecognizerResult {
       Possible,
       Recognized(GestureEvent),
       Failed,
   }
   
   /// Gesture event (high-level)
   #[derive(Clone, Debug)]
   pub enum GestureEvent {
       Tap {
           position: Point<f32, PhysicalPixels>,
           tap_count: u32,
       },
       LongPress {
           position: Point<f32, PhysicalPixels>,
       },
       DragStart {
           position: Point<f32, PhysicalPixels>,
       },
       DragUpdate {
           delta: Offset<f32, PhysicalPixels>,
       },
       DragEnd {
           velocity: Velocity,
       },
       Scale {
           scale: f32,
           focal_point: Point<f32, PhysicalPixels>,
       },
   }
   ```

**Критерии завершения**:
- [ ] Arena resolves conflicts
- [ ] Winner declared correctly
- [ ] Rejected recognizers stop
- [ ] 20+ arena tests

---

#### День 8: Tap & Long Press Recognizers

**Цель**: Basic gesture recognizers

**Задачи**:

1. **Создать `recognizers/tap.rs`**
   ```rust
   /// Tap gesture recognizer
   pub struct TapRecognizer {
       id: RecognizerId,
       state: TapState,
       start_position: Option<Point<f32, PhysicalPixels>>,
       start_time: Option<Instant>,
       tap_count: u32,
       max_drift: f32,
       tap_timeout: Duration,
   }
   
   enum TapState {
       Idle,
       Possible,
       Failed,
   }
   
   impl TapRecognizer {
       pub fn new() -> Self {
           Self {
               id: RecognizerId::new(),
               state: TapState::Idle,
               start_position: None,
               start_time: None,
               tap_count: 0,
               max_drift: 10.0,
               tap_timeout: Duration::from_millis(300),
           }
       }
   }
   
   impl GestureRecognizer for TapRecognizer {
       fn id(&self) -> RecognizerId {
           self.id
       }
       
       fn process_event(
           &mut self,
           event: &PointerEvent,
           _hit_results: &HitTestResult,
       ) -> Option<RecognizerResult> {
           match event {
               PointerEvent::Down { position, .. } => {
                   self.start_position = Some(*position);
                   self.start_time = Some(Instant::now());
                   self.state = TapState::Possible;
                   Some(RecognizerResult::Possible)
               }
               
               PointerEvent::Move { position, .. } => {
                   if let Some(start) = self.start_position {
                       let distance = (*position - start).length();
                       
                       if distance > self.max_drift {
                           self.state = TapState::Failed;
                           return Some(RecognizerResult::Failed);
                       }
                   }
                   
                   Some(RecognizerResult::Possible)
               }
               
               PointerEvent::Up { position, .. } => {
                   if let (Some(start), Some(start_time)) = (self.start_position, self.start_time) {
                       let distance = (*position - start).length();
                       let duration = start_time.elapsed();
                       
                       if distance <= self.max_drift && duration < self.tap_timeout {
                           self.tap_count += 1;
                           
                           return Some(RecognizerResult::Recognized(
                               GestureEvent::Tap {
                                   position: *position,
                                   tap_count: self.tap_count,
                               }
                           ));
                       }
                   }
                   
                   self.state = TapState::Failed;
                   Some(RecognizerResult::Failed)
               }
               
               _ => None,
           }
       }
       
       fn reject(&mut self) {
           self.state = TapState::Failed;
           self.start_position = None;
           self.start_time = None;
       }
   }
   ```

2. **Long Press Recognizer**
   ```rust
   pub struct LongPressRecognizer {
       id: RecognizerId,
       start_position: Option<Point<f32, PhysicalPixels>>,
       start_time: Option<Instant>,
       duration_threshold: Duration,
       max_drift: f32,
   }
   
   impl LongPressRecognizer {
       pub fn new() -> Self {
           Self {
               id: RecognizerId::new(),
               start_position: None,
               start_time: None,
               duration_threshold: Duration::from_millis(500),
               max_drift: 10.0,
           }
       }
   }
   
   impl GestureRecognizer for LongPressRecognizer {
       // Similar to TapRecognizer but checks duration >= threshold
   }
   ```

**Критерии завершения**:
- [ ] Tap recognizer works
- [ ] Double-tap works
- [ ] Long press works
- [ ] 25+ gesture tests

---

#### День 9: Drag & Scale Recognizers

**Цель**: Multi-touch gestures

**Задачи**:

1. **Drag Recognizer**
   ```rust
   pub struct DragRecognizer {
       id: RecognizerId,
       state: DragState,
       start_position: Option<Point<f32, PhysicalPixels>>,
       last_position: Option<Point<f32, PhysicalPixels>>,
       min_distance: f32,
   }
   
   enum DragState {
       Idle,
       Possible,
       Dragging,
       Failed,
   }
   
   impl GestureRecognizer for DragRecognizer {
       fn process_event(
           &mut self,
           event: &PointerEvent,
           _hit_results: &HitTestResult,
       ) -> Option<RecognizerResult> {
           match event {
               PointerEvent::Down { position, .. } => {
                   self.start_position = Some(*position);
                   self.last_position = Some(*position);
                   self.state = DragState::Possible;
                   Some(RecognizerResult::Possible)
               }
               
               PointerEvent::Move { position, .. } => {
                   if let Some(start) = self.start_position {
                       let distance = (*position - start).length();
                       
                       match self.state {
                           DragState::Possible => {
                               if distance >= self.min_distance {
                                   self.state = DragState::Dragging;
                                   return Some(RecognizerResult::Recognized(
                                       GestureEvent::DragStart {
                                           position: *position,
                                       }
                                   ));
                               }
                           }
                           
                           DragState::Dragging => {
                               let delta = *position - self.last_position.unwrap();
                               self.last_position = Some(*position);
                               
                               return Some(RecognizerResult::Recognized(
                                   GestureEvent::DragUpdate { delta }
                               ));
                           }
                           
                           _ => {}
                       }
                   }
                   
                   Some(RecognizerResult::Possible)
               }
               
               PointerEvent::Up { .. } => {
                   if self.state == DragState::Dragging {
                       // Calculate velocity
                       let velocity = Velocity::zero(); // TODO: proper calculation
                       
                       return Some(RecognizerResult::Recognized(
                           GestureEvent::DragEnd { velocity }
                       ));
                   }
                   
                   Some(RecognizerResult::Failed)
               }
               
               _ => None,
           }
       }
       
       fn reject(&mut self) {
           self.state = DragState::Failed;
       }
   }
   ```

2. **Scale (Pinch) Recognizer**
   ```rust
   pub struct ScaleRecognizer {
       id: RecognizerId,
       pointers: HashMap<PointerId, Point<f32, PhysicalPixels>>,
       initial_distance: Option<f32>,
   }
   
   impl GestureRecognizer for ScaleRecognizer {
       fn process_event(
           &mut self,
           event: &PointerEvent,
           _hit_results: &HitTestResult,
       ) -> Option<RecognizerResult> {
           // Multi-touch scale gesture
           // Track 2+ pointers, calculate distance changes
           // Return RecognizerResult::Recognized when scale changes
           todo!("Implement multi-touch scale")
       }
   }
   ```

**Критерии завершения**:
- [ ] Drag works
- [ ] Velocity calculation correct
- [ ] Scale gesture works
- [ ] 20+ drag/scale tests

---

#### День 10: Integration & Testing

**Цель**: End-to-end interaction testing

**Задачи**:

1. **Integration Tests**
   ```rust
   #[test]
   fn test_full_interaction_pipeline() {
       // Create event router
       let focus_manager = Arc::new(FocusManager::global());
       let router = EventRouter::new(focus_manager);
       
       // Register gesture recognizers
       let mut arena = router.gesture_arena.lock();
       arena.add_recognizer(Box::new(TapRecognizer::new()));
       arena.add_recognizer(Box::new(DragRecognizer::new()));
       drop(arena);
       
       // Register event handlers
       let handler = EventHandler::builder()
           .on_pointer(|event| {
               println!("Got pointer event: {:?}", event);
               EventResult::Handled
           })
           .build();
       
       let handler_id = router.register_handler(handler);
       
       // Create mock hittable
       let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
       
       // Simulate tap
       let down = Event::Pointer(PointerEvent::Down {
           pointer_id: PointerId(0),
           position: Point::new(50.0, 50.0),
           button: MouseButton::Left,
           modifiers: Modifiers::default(),
           click_count: 1,
       });
       
       router.route_event(&down, &rect);
       
       let up = Event::Pointer(PointerEvent::Up {
           pointer_id: PointerId(0),
           position: Point::new(50.0, 50.0),
           button: MouseButton::Left,
           modifiers: Modifiers::default(),
       });
       
       router.route_event(&up, &rect);
       
       // Should recognize tap gesture
   }
   ```

2. **Performance Tests**
   ```rust
   #[bench]
   fn bench_hit_testing(b: &mut Bencher) {
       let mut result = HitTestResult::new();
       let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
       let position = Point::new(50.0, 50.0);
       
       b.iter(|| {
           result.clear();
           rect.hit_test(position, &mut result);
       });
   }
   ```

**Критерии завершения**:
- [ ] All components integrated
- [ ] End-to-end tests pass
- [ ] Performance acceptable (<1ms для hit testing)
- [ ] Memory usage reasonable

---

## Критерии Завершения Phase 3

### Обязательные Требования

- [ ] **flui_interaction 0.1.0**
  - [ ] EventRouter routes events correctly
  - [ ] Hit testing accurate и efficient
  - [ ] Focus management works (Tab navigation)
  - [ ] Gesture arena resolves conflicts
  - [ ] Tap, Long Press, Drag, Scale recognizers work
  - [ ] Pointer capture для drag operations
  - [ ] 200+ interaction tests
  - [ ] <1ms hit testing latency
  - [ ] <5ms gesture recognition latency

### Бонусные Цели

- [ ] Custom gesture recognizers extensibility
- [ ] Accessibility focus support
- [ ] Spatial focus navigation (arrow keys)
- [ ] Gesture velocity tracking

---

## Примеры Использования

### Example 1: Basic Event Handling

```rust
use flui_interaction::*;

let focus_manager = Arc::new(FocusManager::global());
let router = EventRouter::new(focus_manager);

// Register handler
let handler = EventHandler::builder()
    .on_pointer(|event| {
        match event {
            PointerEvent::Down { position, .. } => {
                println!("Clicked at {:?}", position);
                EventResult::Handled
            }
            _ => EventResult::Unhandled,
        }
    })
    .build();

let handler_id = router.register_handler(handler);
```

### Example 2: Focus Management

```rust
use flui_interaction::*;

let focus_manager = FocusManager::global();

// Register focusable element
let node = FocusNode::builder()
    .focusable(true)
    .on_focus(|event| {
        println!("Got focus!");
    })
    .on_blur(|event| {
        println!("Lost focus!");
    })
    .build();

let node_id = focus_manager.register_node(node);

// Request focus
focus_manager.request_focus(node_id);

// Check focus
if focus_manager.has_focus(node_id) {
    println!("We have focus!");
}
```

### Example 3: Gesture Recognition

```rust
use flui_interaction::*;

let mut arena = GestureArena::new();

// Add recognizers
arena.add_recognizer(Box::new(TapRecognizer::new()));
arena.add_recognizer(Box::new(DragRecognizer::new()));

// Process pointer events
let hit_results = HitTestResult::new();

let down_event = PointerEvent::Down {
    pointer_id: PointerId(0),
    position: Point::new(100.0, 100.0),
    button: MouseButton::Left,
    modifiers: Modifiers::default(),
    click_count: 1,
};

let gesture_events = arena.process_pointer_event(&down_event, &hit_results);

for gesture in gesture_events {
    match gesture {
        GestureEvent::Tap { position, tap_count } => {
            println!("Tap at {:?}, count: {}", position, tap_count);
        }
        GestureEvent::DragStart { position } => {
            println!("Drag started at {:?}", position);
        }
        _ => {}
    }
}
```

---

## Troubleshooting Guide

### Issue: Events not routed to handlers

**Solution**:
```rust
// Check handler registration
let handler_id = router.register_handler(handler);

// Ensure hit test returns results
let mut result = HitTestResult::new();
root.hit_test(position, &mut result);
assert!(!result.is_empty());
```

### Issue: Focus not working

**Solution**:
```rust
// Ensure node is registered
let node_id = focus_manager.register_node(node);

// Check if node is focusable
let node = FocusNode::builder()
    .focusable(true) // Must be true!
    .build();
```

### Issue: Gestures conflicting

**Solution**:
```rust
// Check arena conflict resolution
// Only one recognizer should win
// Adjust recognizer parameters (max_drift, duration, etc.)
```

---

## Следующие Шаги (Phase 4 Preview)

После завершения Phase 3:

1. **flui_app** - Application lifecycle, multi-window
2. **Full Integration** - Все layers работают вместе
3. **Production Examples** - Демо приложения

---

**Статус**: 🟡 Ready for Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базируется на**: docs/plans/2026-01-22-core-architecture-design.md, PHASE_1_DETAILED_PLAN.md, PHASE_2_DETAILED_PLAN.md
