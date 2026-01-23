# Phase 5: Widget Tree Layer (flui-view) - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущие фазы**: Phase 1-4 должны быть завершены  
> **Референсы**: `.gpui/src/element.rs`, `.gpui/src/view.rs`, Flutter's Widget/Element system  
> **Цель**: Production-ready View/Element architecture с полным lifecycle и rebuild optimization

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui-view
- ✅ Cargo.toml с зависимостями на foundation crates
- ✅ Модульная структура: `element/`, `view/`, `context/`, `owner/`, `tree/`, `child/`, `key/`
- ✅ Базовая архитектура Element system
- ✅ BuildContext, BuildOwner
- ✅ Element lifecycle hooks
- ✅ ViewKey для reconciliation
- ✅ Child storage abstractions

#### Зависимости готовы
- ✅ flui-foundation - базовые abstractions
- ✅ flui-tree - tree utilities
- ✅ flui_types - geometry types
- ✅ flui_rendering - RenderObject system
- ✅ flui_interaction - event handling

### ❌ Что Нужно Доделать / Улучшить

#### Core View System
1. **View Trait** - immutable view declarations
2. **Element Lifecycle** - mount, update, unmount
3. **BuildOwner** - build scheduling and dirty tracking
4. **InheritedWidget** - data propagation down the tree
5. **GlobalKey** - cross-tree references

#### Reconciliation & Optimization
1. **ViewKey-based reconciliation** - efficient tree updates
2. **Element reuse** - minimize allocations
3. **Dirty tracking** - rebuild only what changed
4. **BuildScope** - isolated rebuild zones

#### Advanced Features
1. **State management** - StatefulView pattern
2. **Context propagation** - Theme, MediaQuery, etc.
3. **Lifecycle callbacks** - initState, dispose, didUpdateWidget
4. **Debug utilities** - tree inspection, diagnostics

---

## Детальный План Реализации

### Этап 5.1: Core View Architecture (Неделя 9, Дни 1-3)

#### День 1: View Trait & Stateless Views

**Цель**: Define core View trait with stateless implementation

**Референсы**:
- `.gpui/src/view.rs` - GPUI View pattern
- Flutter Widget class
- Plan `3.7 View Architecture`

**Задачи**:

1. **Создать `view/trait.rs`**
   ```rust
   /// View trait (immutable configuration)
   ///
   /// Views describe WHAT to display, not HOW to display it.
   /// They are immutable and recreated on every build.
   ///
   /// # Design Pattern
   ///
   /// Views follow the Flutter pattern:
   /// - Lightweight, immutable data structures
   /// - No lifecycle (created fresh each frame)
   /// - Single `build()` method that returns child
   ///
   /// Elements provide the lifecycle and mutability.
   pub trait View: 'static {
       /// Element type created by this view
       type Element: Element;
       
       /// Create an element for this view
       fn create_element(&self) -> Self::Element;
   }
   
   /// Marker for stateless views (no internal state)
   pub trait StatelessView: View {
       /// Build the child view
       ///
       /// This is where you compose other views together.
       fn build(&self, context: &BuildContext) -> Box<dyn AnyView>;
   }
   
   /// Type-erased view
   pub trait AnyView: 'static {
       /// Create element (type-erased)
       fn create_element_any(&self) -> Box<dyn AnyElement>;
       
       /// Clone this view
       fn clone_view(&self) -> Box<dyn AnyView>;
       
       /// Type name for debugging
       fn type_name(&self) -> &'static str;
       
       /// View key (if any)
       fn key(&self) -> Option<&ViewKey>;
   }
   
   /// Implement AnyView for all Views
   impl<V: View + Clone> AnyView for V {
       fn create_element_any(&self) -> Box<dyn AnyElement> {
           Box::new(self.create_element())
       }
       
       fn clone_view(&self) -> Box<dyn AnyView> {
           Box::new(self.clone())
       }
       
       fn type_name(&self) -> &'static str {
           std::any::type_name::<V>()
       }
       
       fn key(&self) -> Option<&ViewKey> {
           None // Default: no key
       }
   }
   ```

2. **Stateless View Implementation**
   ```rust
   /// Stateless view element
   ///
   /// Wraps a StatelessView and builds its child.
   pub struct StatelessElement<V: StatelessView> {
       /// View configuration (immutable)
       view: V,
       
       /// Child element (mutable)
       child: Option<ElementId>,
       
       /// Build context
       context: BuildContext,
   }
   
   impl<V: StatelessView> Element for StatelessElement<V> {
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           tracing::debug!("Mounting stateless element: {}", std::any::type_name::<V>());
           
           // Build child view
           let child_view = self.view.build(&self.context);
           
           // Create child element
           let child_element = child_view.create_element_any();
           let child_id = owner.insert_element(child_element);
           
           // Mount child
           owner.mount_element(child_id, Some(self.id()), owner);
           
           self.child = Some(child_id);
       }
       
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner) {
           tracing::debug!("Updating stateless element: {}", std::any::type_name::<V>());
           
           // Downcast to concrete type
           let new_view = new_view.downcast_ref::<V>()
               .expect("View type mismatch");
           
           // Update view configuration
           self.view = new_view.clone();
           
           // Rebuild child
           let new_child_view = self.view.build(&self.context);
           
           // Update or replace child
           if let Some(child_id) = self.child {
               owner.update_element(child_id, new_child_view.as_ref());
           } else {
               // Child was removed, create new
               let child_element = new_child_view.create_element_any();
               let child_id = owner.insert_element(child_element);
               owner.mount_element(child_id, Some(self.id()), owner);
               self.child = Some(child_id);
           }
       }
       
       fn unmount(&mut self, owner: &mut BuildOwner) {
           tracing::debug!("Unmounting stateless element: {}", std::any::type_name::<V>());
           
           // Unmount child
           if let Some(child_id) = self.child.take() {
               owner.unmount_element(child_id);
           }
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
           if let Some(child_id) = self.child {
               visitor(child_id);
           }
       }
       
       fn render_object(&self) -> Option<&dyn RenderObject> {
           None // Stateless elements don't have render objects
       }
   }
   ```

3. **Example Stateless View**
   ```rust
   /// Example: Text view
   #[derive(Clone, Debug)]
   pub struct Text {
       pub data: String,
       pub style: TextStyle,
   }
   
   impl View for Text {
       type Element = TextElement;
       
       fn create_element(&self) -> Self::Element {
           TextElement {
               view: self.clone(),
               render_object_id: None,
           }
       }
   }
   
   /// Text element (has a RenderObject)
   pub struct TextElement {
       view: Text,
       render_object_id: Option<RenderObjectId>,
   }
   
   impl Element for TextElement {
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           // Create RenderText
           let render_text = RenderText::new(
               self.view.data.clone(),
               self.view.style.clone(),
           );
           
           self.render_object_id = Some(owner.insert_render_object(render_text));
       }
       
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner) {
           let new_view = new_view.downcast_ref::<Text>().unwrap();
           
           // Update render object if data changed
           if self.view.data != new_view.data || self.view.style != new_view.style {
               if let Some(id) = self.render_object_id {
                   let render_text = owner.get_render_object_mut(id)
                       .downcast_mut::<RenderText>().unwrap();
                   
                   render_text.set_text(new_view.data.clone());
                   render_text.set_style(new_view.style.clone());
                   render_text.mark_needs_layout();
               }
           }
           
           self.view = new_view.clone();
       }
       
       fn unmount(&mut self, owner: &mut BuildOwner) {
           if let Some(id) = self.render_object_id.take() {
               owner.remove_render_object(id);
           }
       }
       
       fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {
           // Leaf element, no children
       }
       
       fn render_object(&self) -> Option<&dyn RenderObject> {
           self.render_object_id.and_then(|id| owner.get_render_object(id))
       }
   }
   ```

**Критерии завершения**:
- [ ] View trait defined
- [ ] StatelessView works
- [ ] StatelessElement implements lifecycle
- [ ] Example Text view works
- [ ] 25+ view tests

---

#### День 2: Stateful Views & State Management

**Цель**: Implement stateful view pattern with internal state

**Задачи**:

1. **StatefulView Trait**
   ```rust
   /// Stateful view (has internal state)
   ///
   /// State is preserved across rebuilds and managed by the element.
   pub trait StatefulView: View {
       /// State type
       type State: 'static;
       
       /// Create initial state
       fn create_state(&self) -> Self::State;
   }
   
   /// State trait (lifecycle hooks)
   pub trait State: 'static {
       /// View type this state belongs to
       type View: StatefulView<State = Self>;
       
       /// Initialize state (called once on mount)
       fn init_state(&mut self, context: &mut StateContext) {
           // Default: do nothing
       }
       
       /// Called after initState
       fn did_mount(&mut self, context: &mut StateContext) {
           // Default: do nothing
       }
       
       /// Called when view configuration changes
       fn did_update_view(&mut self, old_view: &Self::View, context: &mut StateContext) {
           // Default: do nothing
       }
       
       /// Called before unmount
       fn dispose(&mut self, context: &mut StateContext) {
           // Default: do nothing
       }
       
       /// Build the child view
       fn build(&self, view: &Self::View, context: &BuildContext) -> Box<dyn AnyView>;
   }
   
   /// State context (access to lifecycle methods)
   pub struct StateContext {
       /// Element ID
       element_id: ElementId,
       
       /// Mark this element as dirty (needs rebuild)
       dirty_callback: Box<dyn FnMut()>,
   }
   
   impl StateContext {
       /// Schedule a rebuild
       pub fn set_state<F>(&mut self, f: F)
       where
           F: FnOnce(),
       {
           f();
           (self.dirty_callback)();
       }
   }
   ```

2. **Stateful Element Implementation**
   ```rust
   /// Stateful element
   ///
   /// Manages state across rebuilds.
   pub struct StatefulElement<V: StatefulView> {
       /// View configuration (updated on rebuild)
       view: V,
       
       /// State (persisted across rebuilds)
       state: V::State,
       
       /// Child element
       child: Option<ElementId>,
       
       /// State context
       state_context: StateContext,
       
       /// Build context
       build_context: BuildContext,
   }
   
   impl<V: StatefulView> Element for StatefulElement<V> {
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           tracing::debug!("Mounting stateful element: {}", std::any::type_name::<V>());
           
           // Initialize state
           self.state.init_state(&mut self.state_context);
           self.state.did_mount(&mut self.state_context);
           
           // Build child
           let child_view = self.state.build(&self.view, &self.build_context);
           let child_element = child_view.create_element_any();
           let child_id = owner.insert_element(child_element);
           
           owner.mount_element(child_id, Some(self.id()), owner);
           self.child = Some(child_id);
       }
       
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner) {
           tracing::debug!("Updating stateful element: {}", std::any::type_name::<V>());
           
           let new_view = new_view.downcast_ref::<V>().unwrap();
           let old_view = std::mem::replace(&mut self.view, new_view.clone());
           
           // Notify state of view change
           self.state.did_update_view(&old_view, &mut self.state_context);
           
           // Rebuild child
           let new_child_view = self.state.build(&self.view, &self.build_context);
           
           if let Some(child_id) = self.child {
               owner.update_element(child_id, new_child_view.as_ref());
           }
       }
       
       fn unmount(&mut self, owner: &mut BuildOwner) {
           tracing::debug!("Unmounting stateful element: {}", std::any::type_name::<V>());
           
           // Dispose state
           self.state.dispose(&mut self.state_context);
           
           // Unmount child
           if let Some(child_id) = self.child.take() {
               owner.unmount_element(child_id);
           }
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
           if let Some(child_id) = self.child {
               visitor(child_id);
           }
       }
       
       fn render_object(&self) -> Option<&dyn RenderObject> {
           None
       }
   }
   ```

3. **Example: Counter (Stateful View)**
   ```rust
   /// Counter view (stateful)
   #[derive(Clone, Debug)]
   pub struct Counter {
       pub initial_value: i32,
   }
   
   impl View for Counter {
       type Element = StatefulElement<Self>;
       
       fn create_element(&self) -> Self::Element {
           let state = self.create_state();
           
           StatefulElement {
               view: self.clone(),
               state,
               child: None,
               state_context: StateContext::new(),
               build_context: BuildContext::new(),
           }
       }
   }
   
   impl StatefulView for Counter {
       type State = CounterState;
       
       fn create_state(&self) -> Self::State {
           CounterState {
               count: self.initial_value,
           }
       }
   }
   
   /// Counter state
   pub struct CounterState {
       count: i32,
   }
   
   impl State for CounterState {
       type View = Counter;
       
       fn init_state(&mut self, context: &mut StateContext) {
           tracing::info!("Counter initialized with count: {}", self.count);
       }
       
       fn build(&self, view: &Counter, context: &BuildContext) -> Box<dyn AnyView> {
           Box::new(Column::new(vec![
               Box::new(Text::new(format!("Count: {}", self.count))),
               Box::new(Button::new("Increment").on_click({
                   let mut state_context = context.clone();
                   move || {
                       state_context.set_state(|| {
                           self.count += 1;
                       });
                   }
               })),
           ]))
       }
   }
   ```

**Критерии завершения**:
- [ ] StatefulView trait works
- [ ] State lifecycle hooks work
- [ ] setState triggers rebuild
- [ ] Counter example works
- [ ] 30+ stateful view tests

---

#### День 3: BuildOwner & Build Scheduling

**Цель**: Implement efficient build scheduling with dirty tracking

**Задачи**:

1. **BuildOwner (создать `owner/build_owner.rs`)**
   ```rust
   use dashmap::DashMap;
   use std::sync::Arc;
   use parking_lot::RwLock;
   
   /// Build owner
   ///
   /// Manages the element tree and schedules rebuilds.
   /// Implements dirty tracking to minimize unnecessary work.
   pub struct BuildOwner {
       /// Element storage (Slab for stable IDs)
       elements: Arc<RwLock<Slab<Box<dyn AnyElement>>>>,
       
       /// Dirty elements (need rebuild)
       dirty_elements: Arc<DashMap<ElementId, DirtyReason>>,
       
       /// Root element
       root: Arc<RwLock<Option<ElementId>>>,
       
       /// Build depth (for debugging infinite loops)
       build_depth: Arc<RwLock<usize>>,
       
       /// Focus owner
       focus_owner: Arc<FocusOwner>,
   }
   
   impl BuildOwner {
       pub fn new() -> Self {
           Self {
               elements: Arc::new(RwLock::new(Slab::new())),
               dirty_elements: Arc::new(DashMap::new()),
               root: Arc::new(RwLock::new(None)),
               build_depth: Arc::new(RwLock::new(0)),
               focus_owner: Arc::new(FocusOwner::new()),
           }
       }
       
       /// Set the root element
       pub fn set_root(&self, element_id: ElementId) {
           *self.root.write() = Some(element_id);
           self.mark_needs_build(element_id, DirtyReason::FirstBuild);
       }
       
       /// Mark an element as needing rebuild
       pub fn mark_needs_build(&self, element_id: ElementId, reason: DirtyReason) {
           tracing::trace!("Marking element {:?} dirty: {:?}", element_id, reason);
           self.dirty_elements.insert(element_id, reason);
       }
       
       /// Build all dirty elements
       pub fn build_scope<F, R>(&self, f: F) -> R
       where
           F: FnOnce() -> R,
       {
           // Increment build depth
           {
               let mut depth = self.build_depth.write();
               *depth += 1;
               
               if *depth > 100 {
                   panic!("Build depth exceeded 100 - possible infinite loop");
               }
           }
           
           let result = f();
           
           // Build dirty elements
           self.flush_build();
           
           // Decrement build depth
           {
               let mut depth = self.build_depth.write();
               *depth -= 1;
           }
           
           result
       }
       
       /// Flush all dirty elements
       fn flush_build(&self) {
           while !self.dirty_elements.is_empty() {
               // Collect dirty elements (sorted by depth for top-down rebuild)
               let mut dirty: Vec<(ElementId, DirtyReason)> = self.dirty_elements
                   .iter()
                   .map(|entry| (*entry.key(), *entry.value()))
                   .collect();
               
               // Clear dirty set
               self.dirty_elements.clear();
               
               // Sort by tree depth (top-down)
               dirty.sort_by_key(|(id, _)| self.element_depth(*id));
               
               // Rebuild each element
               for (element_id, reason) in dirty {
                   self.rebuild_element(element_id, reason);
               }
           }
       }
       
       fn rebuild_element(&self, element_id: ElementId, reason: DirtyReason) {
           tracing::debug!("Rebuilding element {:?}: {:?}", element_id, reason);
           
           let elements = self.elements.read();
           let element = elements.get(element_id.get() - 1)
               .expect("Element not found");
           
           // Trigger rebuild
           match reason {
               DirtyReason::FirstBuild => {
                   // Mount
                   element.mount(None, self);
               }
               DirtyReason::ParentUpdate => {
                   // Parent updated, may need to update this element
                   // TODO: Get new view from parent
               }
               DirtyReason::SetState => {
                   // State changed, rebuild
                   element.perform_rebuild(self);
               }
           }
       }
       
       fn element_depth(&self, element_id: ElementId) -> usize {
           // Calculate depth in tree
           let mut depth = 0;
           let mut current = element_id;
           
           while let Some(parent) = self.get_parent(current) {
               depth += 1;
               current = parent;
           }
           
           depth
       }
       
       /// Insert a new element
       pub fn insert_element(&self, element: Box<dyn AnyElement>) -> ElementId {
           let mut elements = self.elements.write();
           let index = elements.insert(element);
           ElementId::new(index + 1) // +1 for NonZeroUsize
       }
       
       /// Get element by ID
       pub fn get_element(&self, id: ElementId) -> Option<Arc<dyn AnyElement>> {
           let elements = self.elements.read();
           elements.get(id.get() - 1).map(|e| Arc::clone(e))
       }
       
       /// Remove element
       pub fn remove_element(&self, id: ElementId) {
           let mut elements = self.elements.write();
           elements.remove(id.get() - 1);
       }
   }
   
   /// Reason for marking element dirty
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum DirtyReason {
       /// First build (mount)
       FirstBuild,
       
       /// Parent was updated
       ParentUpdate,
       
       /// setState() was called
       SetState,
   }
   ```

**Критерии завершения**:
- [ ] BuildOwner manages element tree
- [ ] Dirty tracking works
- [ ] Top-down rebuild order
- [ ] Build depth protection
- [ ] 40+ build owner tests

---

### Этап 5.2: Reconciliation & Keys (Неделя 9-10, Дни 4-6)

#### День 4: ViewKey & Reconciliation Algorithm

**Цель**: Implement key-based reconciliation for efficient updates

**Задачи**:

1. **ViewKey (уже существует, улучшить)**
   ```rust
   /// View key (for reconciliation)
   ///
   /// Keys allow elements to be preserved across rebuilds even when
   /// their position in the tree changes.
   #[derive(Clone, Debug, PartialEq, Eq, Hash)]
   pub enum ViewKey {
       /// Value key (based on data)
       Value(String),
       
       /// Object key (unique object reference)
       Object(usize),
       
       /// Global key (application-wide unique)
       Global(GlobalKey),
   }
   
   /// Global key (cross-tree reference)
   #[derive(Clone, Debug, PartialEq, Eq, Hash)]
   pub struct GlobalKey {
       id: u64,
   }
   
   impl GlobalKey {
       pub fn new() -> Self {
           use std::sync::atomic::{AtomicU64, Ordering};
           static NEXT_ID: AtomicU64 = AtomicU64::new(1);
           
           Self {
               id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
           }
       }
   }
   ```

2. **Reconciliation Algorithm**
   ```rust
   /// Reconcile children (update existing, add new, remove old)
   pub fn reconcile_children(
       old_children: &[ElementId],
       new_views: &[Box<dyn AnyView>],
       owner: &mut BuildOwner,
   ) -> Vec<ElementId> {
       let mut new_children = Vec::with_capacity(new_views.len());
       
       // Build key maps
       let old_keyed: HashMap<&ViewKey, ElementId> = old_children
           .iter()
           .filter_map(|&id| {
               let element = owner.get_element(id)?;
               let key = element.view().key()?;
               Some((key, id))
           })
           .collect();
       
       let mut old_unkeyed: Vec<ElementId> = old_children
           .iter()
           .filter(|&&id| {
               owner.get_element(id)
                   .and_then(|e| e.view().key())
                   .is_none()
           })
           .copied()
           .collect();
       
       // Process new views
       for new_view in new_views {
           let element_id = if let Some(key) = new_view.key() {
               // Keyed element: try to reuse
               if let Some(&old_id) = old_keyed.get(key) {
                   // Reuse and update
                   owner.update_element(old_id, new_view.as_ref());
                   old_id
               } else {
                   // Create new
                   let element = new_view.create_element_any();
                   let id = owner.insert_element(element);
                   owner.mount_element(id, parent, owner);
                   id
               }
           } else {
               // Unkeyed element: try to reuse by position
               if let Some(old_id) = old_unkeyed.pop() {
                   // Check if types match
                   let old_element = owner.get_element(old_id).unwrap();
                   
                   if old_element.view().type_name() == new_view.type_name() {
                       // Same type, update
                       owner.update_element(old_id, new_view.as_ref());
                       old_id
                   } else {
                       // Different type, replace
                       owner.unmount_element(old_id);
                       
                       let element = new_view.create_element_any();
                       let id = owner.insert_element(element);
                       owner.mount_element(id, parent, owner);
                       id
                   }
               } else {
                   // No more old elements, create new
                   let element = new_view.create_element_any();
                   let id = owner.insert_element(element);
                   owner.mount_element(id, parent, owner);
                   id
               }
           };
           
           new_children.push(element_id);
       }
       
       // Remove unused old elements
       for &old_id in &old_unkeyed {
           owner.unmount_element(old_id);
       }
       
       for (&key, &old_id) in &old_keyed {
           // If not reused, remove
           if !new_children.contains(&old_id) {
               owner.unmount_element(old_id);
           }
       }
       
       new_children
   }
   ```

**Критерии завершения**:
- [ ] ViewKey system works
- [ ] GlobalKey works
- [ ] Reconciliation algorithm works
- [ ] Element reuse verified
- [ ] 35+ reconciliation tests

---

#### День 5: BuildContext & InheritedWidget

**Цель**: Context propagation down the tree

**Задачи**:

1. **BuildContext (улучшить существующий)**
   ```rust
   /// Build context
   ///
   /// Provides access to ancestor data and services during build.
   pub struct BuildContext {
       /// Element ID
       element_id: ElementId,
       
       /// Build owner
       owner: Weak<BuildOwner>,
       
       /// Inherited widget cache
       inherited_cache: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
   }
   
   impl BuildContext {
       /// Find ancestor inherited widget
       pub fn depend_on_inherited_widget<T: InheritedWidget>(&self) -> Option<Arc<T>> {
           let type_id = TypeId::of::<T>();
           
           // Check cache first
           {
               let cache = self.inherited_cache.read();
               if let Some(cached) = cache.get(&type_id) {
                   return Some(Arc::clone(cached).downcast::<T>().ok()?);
               }
           }
           
           // Walk up tree to find inherited widget
           let owner = self.owner.upgrade()?;
           let mut current = self.element_id;
           
           loop {
               let element = owner.get_element(current)?;
               
               // Check if this element is an InheritedElement<T>
               if let Some(inherited) = element.as_inherited::<T>() {
                   // Cache and return
                   let arc_inherited = Arc::new(inherited.clone());
                   self.inherited_cache.write()
                       .insert(type_id, Arc::clone(&arc_inherited) as Arc<dyn Any + Send + Sync>);
                   
                   return Some(arc_inherited);
               }
               
               // Move to parent
               current = owner.get_parent(current)?;
           }
       }
       
       /// Access theme
       pub fn theme(&self) -> Arc<Theme> {
           self.depend_on_inherited_widget::<Theme>()
               .unwrap_or_else(|| Arc::new(Theme::default()))
       }
       
       /// Access media query
       pub fn media_query(&self) -> Arc<MediaQuery> {
           self.depend_on_inherited_widget::<MediaQuery>()
               .unwrap_or_else(|| Arc::new(MediaQuery::default()))
       }
   }
   ```

2. **InheritedWidget**
   ```rust
   /// Inherited widget (data propagation)
   ///
   /// Provides data to all descendants without explicit passing.
   pub trait InheritedWidget: 'static + Clone {
       /// Check if this widget should notify dependents
       fn should_notify(&self, old: &Self) -> bool {
           // Default: always notify
           true
       }
   }
   
   /// Inherited view
   #[derive(Clone)]
   pub struct Inherited<T: InheritedWidget> {
       pub data: T,
       pub child: Box<dyn AnyView>,
   }
   
   impl<T: InheritedWidget> View for Inherited<T> {
       type Element = InheritedElement<T>;
       
       fn create_element(&self) -> Self::Element {
           InheritedElement {
               view: self.clone(),
               child: None,
               dependents: Arc::new(DashSet::new()),
           }
       }
   }
   
   /// Inherited element
   pub struct InheritedElement<T: InheritedWidget> {
       view: Inherited<T>,
       child: Option<ElementId>,
       
       /// Elements that depend on this inherited widget
       dependents: Arc<DashSet<ElementId>>,
   }
   
   impl<T: InheritedWidget> Element for InheritedElement<T> {
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           // Build child
           let child_element = self.view.child.create_element_any();
           let child_id = owner.insert_element(child_element);
           owner.mount_element(child_id, Some(self.id()), owner);
           self.child = Some(child_id);
       }
       
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner) {
           let new_view = new_view.downcast_ref::<Inherited<T>>().unwrap();
           
           // Check if data changed
           let should_notify = new_view.data.should_notify(&self.view.data);
           
           self.view = new_view.clone();
           
           if should_notify {
               // Notify all dependents
               for dependent_id in self.dependents.iter() {
                   owner.mark_needs_build(*dependent_id, DirtyReason::InheritedChanged);
               }
           }
           
           // Update child
           if let Some(child_id) = self.child {
               owner.update_element(child_id, &*self.view.child);
           }
       }
       
       fn unmount(&mut self, owner: &mut BuildOwner) {
           if let Some(child_id) = self.child.take() {
               owner.unmount_element(child_id);
           }
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
           if let Some(child_id) = self.child {
               visitor(child_id);
           }
       }
       
       fn render_object(&self) -> Option<&dyn RenderObject> {
           None
       }
       
       fn as_inherited<U: InheritedWidget>(&self) -> Option<&U> {
           if TypeId::of::<T>() == TypeId::of::<U>() {
               Some(unsafe { &*(&self.view.data as *const T as *const U) })
           } else {
               None
           }
       }
       
       /// Register a dependent
       pub fn add_dependent(&self, element_id: ElementId) {
           self.dependents.insert(element_id);
       }
   }
   ```

3. **Example: Theme**
   ```rust
   /// Theme (inherited widget)
   #[derive(Clone, Debug, PartialEq)]
   pub struct Theme {
       pub primary_color: Color,
       pub background_color: Color,
       pub text_color: Color,
       pub font_size: f32,
   }
   
   impl InheritedWidget for Theme {
       fn should_notify(&self, old: &Self) -> bool {
           self != old
       }
   }
   
   // Usage:
   let app = Inherited {
       data: Theme {
           primary_color: Color::BLUE,
           background_color: Color::WHITE,
           text_color: Color::BLACK,
           font_size: 14.0,
       },
       child: Box::new(MyApp::new()),
   };
   
   // In MyApp:
   fn build(&self, context: &BuildContext) -> Box<dyn AnyView> {
       let theme = context.theme();
       
       Box::new(Text {
           data: "Hello".into(),
           style: TextStyle {
               color: theme.text_color,
               font_size: theme.font_size,
               ..Default::default()
           },
       })
   }
   ```

**Критерии завершения**:
- [ ] BuildContext works
- [ ] InheritedWidget works
- [ ] Dependent tracking works
- [ ] Theme example works
- [ ] 30+ context tests

---

#### День 6: Builder Views & Composition

**Цель**: Helper views for common patterns

**Задачи**:

1. **Builder View**
   ```rust
   /// Builder view (function-based)
   pub struct Builder {
       builder: Arc<dyn Fn(&BuildContext) -> Box<dyn AnyView> + Send + Sync>,
   }
   
   impl Builder {
       pub fn new<F>(builder: F) -> Self
       where
           F: Fn(&BuildContext) -> Box<dyn AnyView> + Send + Sync + 'static,
       {
           Self {
               builder: Arc::new(builder),
           }
       }
   }
   
   impl View for Builder {
       type Element = BuilderElement;
       
       fn create_element(&self) -> Self::Element {
           BuilderElement {
               view: self.clone(),
               child: None,
           }
       }
   }
   
   impl Clone for Builder {
       fn clone(&self) -> Self {
           Self {
               builder: Arc::clone(&self.builder),
           }
       }
   }
   ```

2. **Container Views**
   ```rust
   /// Center view
   #[derive(Clone)]
   pub struct Center {
       pub child: Box<dyn AnyView>,
   }
   
   impl View for Center {
       type Element = CenterElement;
       
       fn create_element(&self) -> Self::Element {
           CenterElement {
               view: self.clone(),
               child: None,
               render_object_id: None,
           }
       }
   }
   
   /// Padding view
   #[derive(Clone)]
   pub struct Padding {
       pub padding: EdgeInsets,
       pub child: Box<dyn AnyView>,
   }
   
   /// SizedBox view
   #[derive(Clone)]
   pub struct SizedBox {
       pub width: Option<f32>,
       pub height: Option<f32>,
       pub child: Option<Box<dyn AnyView>>,
   }
   
   /// Flexible view (for Flex children)
   #[derive(Clone)]
   pub struct Flexible {
       pub flex: u32,
       pub child: Box<dyn AnyView>,
   }
   
   /// Expanded view (Flexible with flex=1)
   #[derive(Clone)]
   pub struct Expanded {
       pub child: Box<dyn AnyView>,
   }
   
   impl Expanded {
       pub fn new(child: Box<dyn AnyView>) -> Self {
           Self { child }
       }
   }
   ```

3. **Layout Views**
   ```rust
   /// Row view (horizontal layout)
   #[derive(Clone)]
   pub struct Row {
       pub children: Vec<Box<dyn AnyView>>,
       pub main_axis_alignment: MainAxisAlignment,
       pub cross_axis_alignment: CrossAxisAlignment,
       pub main_axis_size: MainAxisSize,
   }
   
   /// Column view (vertical layout)
   #[derive(Clone)]
   pub struct Column {
       pub children: Vec<Box<dyn AnyView>>,
       pub main_axis_alignment: MainAxisAlignment,
       pub cross_axis_alignment: CrossAxisAlignment,
       pub main_axis_size: MainAxisSize,
   }
   
   /// Stack view (layered layout)
   #[derive(Clone)]
   pub struct Stack {
       pub children: Vec<Box<dyn AnyView>>,
       pub alignment: Alignment,
       pub fit: StackFit,
   }
   ```

**Критерии завершения**:
- [ ] Builder view works
- [ ] Container views work
- [ ] Layout views work
- [ ] Composition examples work
- [ ] 25+ composition tests

---

### Этап 5.3: Advanced Features (Неделя 10, Дни 7-10)

#### День 7: GlobalKey & Cross-Tree References

**Цель**: Implement GlobalKey for direct element access

**Задачи**:

1. **GlobalKey Registry**
   ```rust
   /// Global key registry
   ///
   /// Maps global keys to element IDs.
   pub struct GlobalKeyRegistry {
       keys: Arc<DashMap<GlobalKey, ElementId>>,
   }
   
   impl GlobalKeyRegistry {
       pub fn new() -> Self {
           Self {
               keys: Arc::new(DashMap::new()),
           }
       }
       
       /// Register a global key
       pub fn register(&self, key: GlobalKey, element_id: ElementId) {
           if let Some(old_id) = self.keys.insert(key.clone(), element_id) {
               tracing::warn!("GlobalKey {:?} already registered (old: {:?}, new: {:?})",
                   key, old_id, element_id);
           }
       }
       
       /// Unregister a global key
       pub fn unregister(&self, key: &GlobalKey) {
           self.keys.remove(key);
       }
       
       /// Get element ID for a global key
       pub fn get(&self, key: &GlobalKey) -> Option<ElementId> {
           self.keys.get(key).map(|entry| *entry.value())
       }
       
       /// Get current state for a stateful element
       pub fn current_state<S: State>(&self, key: &GlobalKey) -> Option<Arc<S>> {
           let element_id = self.get(key)?;
           // Get element and extract state
           // TODO: Access element's state
           None
       }
   }
   ```

2. **Usage Example**
   ```rust
   // Create global key
   let counter_key = GlobalKey::new();
   
   // Use in view
   let app = Column::new(vec![
       Box::new(Counter {
           key: Some(ViewKey::Global(counter_key.clone())),
           initial_value: 0,
       }),
       Box::new(Button::new("Reset Counter").on_click({
           let key = counter_key.clone();
           move || {
               // Access counter state via global key
               if let Some(state) = global_key_registry.current_state::<CounterState>(&key) {
                   state.reset();
               }
           }
       })),
   ]);
   ```

**Критерии завершения**:
- [ ] GlobalKey registry works
- [ ] Element access via GlobalKey works
- [ ] State access via GlobalKey works
- [ ] 20+ GlobalKey tests

---

#### День 8: Performance Optimization

**Цель**: Optimize rebuild performance

**Задачи**:

1. **Const Views (Optimization)**
   ```rust
   /// Const view (never rebuilds)
   ///
   /// Wraps a view and caches its element permanently.
   pub struct Const {
       child: Box<dyn AnyView>,
   }
   
   impl Const {
       pub fn new(child: Box<dyn AnyView>) -> Self {
           Self { child }
       }
   }
   
   impl View for Const {
       type Element = ConstElement;
       
       fn create_element(&self) -> Self::Element {
           let child_element = self.child.create_element_any();
           
           ConstElement {
               child_element: Arc::new(Mutex::new(child_element)),
           }
       }
   }
   
   pub struct ConstElement {
       child_element: Arc<Mutex<Box<dyn AnyElement>>>,
   }
   
   impl Element for ConstElement {
       fn mount(&mut self, parent: Option<ElementId>, owner: &mut BuildOwner) {
           // Mount child element
           let mut child = self.child_element.lock();
           child.mount(parent, owner);
       }
       
       fn update(&mut self, new_view: &dyn AnyView, owner: &mut BuildOwner) {
           // NEVER update - child is const
       }
       
       fn unmount(&mut self, owner: &mut BuildOwner) {
           let mut child = self.child_element.lock();
           child.unmount(owner);
       }
       
       fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
           let child = self.child_element.lock();
           child.visit_children(visitor);
       }
       
       fn render_object(&self) -> Option<&dyn RenderObject> {
           None
       }
   }
   ```

2. **RepaintBoundary (Optimization)**
   ```rust
   /// Repaint boundary
   ///
   /// Isolates repaints to this subtree (doesn't propagate up).
   pub struct RepaintBoundary {
       child: Box<dyn AnyView>,
   }
   
   impl RepaintBoundary {
       pub fn new(child: Box<dyn AnyView>) -> Self {
           Self { child }
       }
   }
   ```

3. **ShouldRebuild (Optimization)**
   ```rust
   /// Trait for custom rebuild logic
   pub trait ShouldRebuild {
       /// Check if this view should rebuild given the old view
       fn should_rebuild(&self, old: &Self) -> bool;
   }
   
   // Example: Only rebuild if data changed
   impl ShouldRebuild for Text {
       fn should_rebuild(&self, old: &Self) -> bool {
           self.data != old.data || self.style != old.style
       }
   }
   ```

**Критерии завершения**:
- [ ] Const view works
- [ ] RepaintBoundary works
- [ ] ShouldRebuild works
- [ ] Performance benchmarks show improvement
- [ ] 15+ optimization tests

---

#### День 9: Debug Utilities & Inspector

**Цель**: Development tools for debugging view tree

**Задачи**:

1. **Tree Dumper**
   ```rust
   /// Dump element tree to string
   pub fn dump_tree(owner: &BuildOwner, root: ElementId) -> String {
       let mut output = String::new();
       dump_tree_recursive(owner, root, 0, &mut output);
       output
   }
   
   fn dump_tree_recursive(
       owner: &BuildOwner,
       element_id: ElementId,
       depth: usize,
       output: &mut String,
   ) {
       let element = owner.get_element(element_id).unwrap();
       let indent = "  ".repeat(depth);
       
       output.push_str(&format!("{}|- {} (id: {:?})\n",
           indent,
           element.view().type_name(),
           element_id,
       ));
       
       element.visit_children(&mut |child_id| {
           dump_tree_recursive(owner, child_id, depth + 1, output);
       });
   }
   ```

2. **Element Inspector**
   ```rust
   /// Element inspector (for devtools)
   pub struct ElementInspector {
       owner: Arc<BuildOwner>,
   }
   
   impl ElementInspector {
       pub fn inspect_element(&self, element_id: ElementId) -> ElementInfo {
           let element = self.owner.get_element(element_id).unwrap();
           
           ElementInfo {
               id: element_id,
               type_name: element.view().type_name().to_string(),
               key: element.view().key().map(|k| format!("{:?}", k)),
               depth: self.calculate_depth(element_id),
               children_count: self.count_children(element_id),
               has_render_object: element.render_object().is_some(),
           }
       }
       
       fn calculate_depth(&self, element_id: ElementId) -> usize {
           // Calculate depth
           0
       }
       
       fn count_children(&self, element_id: ElementId) -> usize {
           let mut count = 0;
           let element = self.owner.get_element(element_id).unwrap();
           element.visit_children(&mut |_| count += 1);
           count
       }
   }
   
   #[derive(Debug)]
   pub struct ElementInfo {
       pub id: ElementId,
       pub type_name: String,
       pub key: Option<String>,
       pub depth: usize,
       pub children_count: usize,
       pub has_render_object: bool,
   }
   ```

**Критерии завершения**:
- [ ] Tree dumper works
- [ ] Element inspector works
- [ ] Debug utilities integrated
- [ ] 10+ debug tests

---

#### День 10: Integration Testing & Documentation

**Цель**: Production readiness

**Задачи**:

1. **Integration Tests**
   ```rust
   #[test]
   fn test_full_app_lifecycle() {
       let owner = BuildOwner::new();
       
       // Create app
       let app = Counter { initial_value: 0 };
       let root_element = app.create_element();
       let root_id = owner.insert_element(Box::new(root_element));
       
       owner.set_root(root_id);
       
       // Build
       owner.build_scope(|| {
           // Initial build
       });
       
       // Simulate user interaction
       // ... setState
       
       // Rebuild
       owner.build_scope(|| {
           // Rebuild after state change
       });
       
       // Verify tree
       let tree_dump = dump_tree(&owner, root_id);
       assert!(tree_dump.contains("Counter"));
   }
   ```

2. **Documentation**
   - README.md for flui-view
   - Architecture documentation
   - API docs for all public items
   - Examples for common patterns
   - Migration guide from other frameworks

**Критерии завершения**:
- [ ] All tests pass
- [ ] cargo doc builds
- [ ] README complete
- [ ] Examples documented
- [ ] Architecture documented

---

## Критерии Завершения Phase 5

### Обязательные Требования

- [ ] **flui-view 0.1.0**
  - [ ] View trait works
  - [ ] StatelessView works
  - [ ] StatefulView works with state management
  - [ ] BuildOwner manages element tree
  - [ ] Dirty tracking and efficient rebuilds
  - [ ] ViewKey-based reconciliation
  - [ ] InheritedWidget for data propagation
  - [ ] GlobalKey for cross-tree references
  - [ ] Builder and container views
  - [ ] 200+ view system tests
  - [ ] All examples run successfully

### Бонусные Цели

- [ ] Const view optimization
- [ ] RepaintBoundary optimization
- [ ] ShouldRebuild custom logic
- [ ] Element inspector
- [ ] Performance benchmarks

---

## Примеры Использования

### Example 1: Stateless View

```rust
use flui_view::*;

#[derive(Clone)]
struct Greeting {
    name: String,
}

impl View for Greeting {
    type Element = StatelessElement<Self>;
    
    fn create_element(&self) -> Self::Element {
        StatelessElement::new(self.clone())
    }
}

impl StatelessView for Greeting {
    fn build(&self, context: &BuildContext) -> Box<dyn AnyView> {
        Box::new(Text::new(format!("Hello, {}!", self.name)))
    }
}
```

### Example 2: Stateful Counter

```rust
#[derive(Clone)]
struct Counter {
    initial: i32,
}

impl StatefulView for Counter {
    type State = CounterState;
    
    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

struct CounterState {
    count: i32,
}

impl State for CounterState {
    type View = Counter;
    
    fn build(&self, view: &Counter, context: &BuildContext) -> Box<dyn AnyView> {
        Box::new(Column::new(vec![
            Box::new(Text::new(format!("Count: {}", self.count))),
            Box::new(Button::new("++").on_click({
                let mut ctx = context.clone();
                move || {
                    ctx.set_state(|| {
                        self.count += 1;
                    });
                }
            })),
        ]))
    }
}
```

### Example 3: InheritedWidget (Theme)

```rust
let app = Inherited {
    data: Theme {
        primary_color: Color::BLUE,
        text_color: Color::BLACK,
    },
    child: Box::new(MyApp::new()),
};

// In MyApp:
fn build(&self, context: &BuildContext) -> Box<dyn AnyView> {
    let theme = context.theme();
    
    Box::new(Text {
        data: "Themed Text".into(),
        style: TextStyle {
            color: theme.text_color,
            ..Default::default()
        },
    })
}
```

---

## Troubleshooting Guide

### Issue: Infinite rebuild loop

**Solution**:
```rust
// Check build depth
// BuildOwner panics at depth > 100

// Ensure setState() is not called during build:
fn build(&self, view: &MyView, context: &BuildContext) -> Box<dyn AnyView> {
    // ❌ WRONG: setState during build
    // context.set_state(|| { ... });
    
    // ✅ CORRECT: setState in event handler
    Box::new(Button::new("Click").on_click({
        let mut ctx = context.clone();
        move || {
            ctx.set_state(|| { ... });
        }
    }))
}
```

### Issue: Element not updating

**Solution**:
```rust
// Ensure ViewKey is used for keyed reconciliation
let items = vec![
    Box::new(Text::new("Item 1").with_key(ViewKey::Value("1".into()))),
    Box::new(Text::new("Item 2").with_key(ViewKey::Value("2".into()))),
];

// Or implement ShouldRebuild
impl ShouldRebuild for MyView {
    fn should_rebuild(&self, old: &Self) -> bool {
        self.important_field != old.important_field
    }
}
```

---

## Следующие Шаги (Production)

После завершения Phase 5:

1. **Phase 6: flui_rendering** - RenderObject system
2. **Phase 7: flui-scheduler** - Frame scheduling
3. **Phase 8: flui_widgets** - Widget library (Button, TextField, etc.)
4. **Production Apps** - Real applications

---

**Статус**: 🟡 Ready for Implementation  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базируется на**: GPUI element.rs/view.rs + Flutter Widget/Element system + original architecture design
