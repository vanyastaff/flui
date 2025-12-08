# 📋 Полный план рефакторинга flui-core для TreeCoordinator

## 🎯 Цель

Интегрировать **четырех-уровневую архитектуру** (ViewTree, ElementTree, RenderTree, LayerTree) в flui-core через TreeCoordinator.

## 📊 Scope изменений

**Затронутые крейты:**
- ✏️ `flui_core/src/pipeline/` - основные изменения
- ✏️ `flui-pipeline/src/context.rs` - обновление BuildContext
- ✏️ `flui-element/src/` - удаление deprecated кода
- 📖 `docs/` - обновление документации

**Не затрагиваются:**
- ✅ `flui-foundation` - уже готово (ViewId, ElementId, RenderId, LayerId)
- ✅ `flui-tree` - абстрактные traits готовы
- ✅ `flui-view/src/tree.rs` - ViewTree готово
- ✅ `flui-rendering/src/tree.rs` - RenderTree готово
- ✅ `flui-engine/src/tree.rs` - LayerTree готово

---

## 🗂️ Фаза 0: Подготовка (1-2 часа)

### 0.1 Создать feature branch

```bash
git checkout -b refactor/integrate-tree-coordinator
git push -u origin refactor/integrate-tree-coordinator
```

### 0.2 Бэкап текущего состояния

```bash
# Сохранить текущие тесты
cargo test --all > test_baseline.txt

# Сохранить текущую структуру
tree crates/flui_core/src/pipeline > structure_before.txt
```

### 0.3 Создать checklist файл

```markdown
# TreeCoordinator Integration Checklist

## Phase 1: TreeCoordinator refactor
- [ ] Remove generic `<E>` from TreeCoordinator
- [ ] Add concrete ElementTree field
- [ ] Update all accessors
- [ ] Update tests

## Phase 2: PipelineOwner integration
- [ ] Replace `tree: Arc<RwLock<ElementTree>>` with `tree_coord`
- [ ] Add accessor methods
- [ ] Update all usages
- [ ] Verify compilation

## Phase 3: BuildPipeline refactor
- [ ] Update rebuild_element() to use TreeCoordinator
- [ ] Add process_child() helper
- [ ] Update can_reuse() logic
- [ ] Update reconciliation

## Phase 4: PipelineBuildContext refactor
- [ ] Add coordinator field
- [ ] Implement view_object() access
- [ ] Implement render_object() access
- [ ] Update for_child()

## Phase 5: FrameCoordinator updates
- [ ] Pass TreeCoordinator to build pipeline
- [ ] Update build_frame()
- [ ] Update flush_build()

## Phase 6: Clean up deprecated code
- [ ] Remove Element deprecated methods
- [ ] Remove IntoElement deprecated impls
- [ ] Update migration guides

## Phase 7: Tests
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Add new TreeCoordinator tests
- [ ] Performance benchmarks

## Phase 8: Documentation
- [ ] Update architecture docs
- [ ] Add migration guide
- [ ] Update examples
```

---

## 🔧 Фаза 1: Рефакторинг TreeCoordinator (2-3 часа)

### 1.1 Убрать дженерик из TreeCoordinator

**Файл:** `crates/flui_core/src/pipeline/tree_coordinator.rs`

**Изменения:**

```rust
// ========== BEFORE ==========
pub struct TreeCoordinator<E> {
    views: ViewTree,
    elements: E,  // ❌ Дженерик
    render_objects: RenderTree,
    layers: LayerTree,
    // ...
}

// ========== AFTER ==========
/// Coordinates the four separate trees in FLUI's architecture.
///
/// # Architecture
///
/// ```text
/// TreeCoordinator
///   ├── views: ViewTree           (ViewObjects storage)
///   ├── elements: ElementTree     (Element storage with ID refs)
///   ├── render_objects: RenderTree (RenderObjects storage)
///   └── layers: LayerTree         (Compositor layers)
/// ```
pub struct TreeCoordinator {
    // ========== Four Trees ==========
    /// ViewTree - stores ViewObjects (immutable view definitions)
    views: ViewTree,

    /// ElementTree - stores Elements with ID references
    elements: ElementTree,  // ✅ Конкретный тип

    /// RenderTree - stores RenderObjects (layout and paint logic)
    render_objects: RenderTree,

    /// LayerTree - stores compositor layers
    layers: LayerTree,

    // ========== Dirty Tracking (Flutter pattern) ==========
    /// Elements that need build (view changed)
    needs_build: HashSet<ElementId>,

    /// Elements that need layout (constraints changed)
    needs_layout: HashSet<ElementId>,

    /// Elements that need paint (visual properties changed)
    needs_paint: HashSet<ElementId>,

    /// Elements that need compositing update (layer structure changed)
    needs_compositing: HashSet<ElementId>,

    /// Root element ID
    root: Option<ElementId>,
}

impl TreeCoordinator {
    /// Creates a new TreeCoordinator with empty trees.
    pub fn new() -> Self {
        Self {
            views: ViewTree::new(),
            elements: ElementTree::new(),
            render_objects: RenderTree::new(),
            layers: LayerTree::new(),
            needs_build: HashSet::new(),
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing: HashSet::new(),
            root: None,
        }
    }

    /// Creates a TreeCoordinator with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            views: ViewTree::with_capacity(capacity),
            elements: ElementTree::with_capacity(capacity),
            render_objects: RenderTree::with_capacity(capacity),
            layers: LayerTree::with_capacity(capacity),
            needs_build: HashSet::with_capacity(capacity),
            needs_layout: HashSet::with_capacity(capacity),
            needs_paint: HashSet::with_capacity(capacity),
            needs_compositing: HashSet::with_capacity(capacity),
            root: None,
        }
    }
}

// ========== Tree Access ==========
impl TreeCoordinator {
    /// Returns a reference to the ViewTree.
    #[inline]
    pub fn views(&self) -> &ViewTree {
        &self.views
    }

    /// Returns a mutable reference to the ViewTree.
    #[inline]
    pub fn views_mut(&mut self) -> &mut ViewTree {
        &mut self.views
    }

    /// Returns a reference to the ElementTree.
    #[inline]
    pub fn elements(&self) -> &ElementTree {
        &self.elements
    }

    /// Returns a mutable reference to the ElementTree.
    #[inline]
    pub fn elements_mut(&mut self) -> &mut ElementTree {
        &mut self.elements
    }

    /// Returns a reference to the RenderTree.
    #[inline]
    pub fn render_objects(&self) -> &RenderTree {
        &self.render_objects
    }

    /// Returns a mutable reference to the RenderTree.
    #[inline]
    pub fn render_objects_mut(&mut self) -> &mut RenderTree {
        &mut self.render_objects
    }

    /// Returns a reference to the LayerTree.
    #[inline]
    pub fn layers(&self) -> &LayerTree {
        &self.layers
    }

    /// Returns a mutable reference to the LayerTree.
    #[inline]
    pub fn layers_mut(&mut self) -> &mut LayerTree {
        &mut self.layers
    }
}

// ========== Root Management ==========
impl TreeCoordinator {
    /// Gets the root element ID.
    #[inline]
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Sets the root element ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ElementId>) {
        self.root = root;
    }
}

// ========== Dirty Tracking (Flutter PipelineOwner pattern) ==========
impl TreeCoordinator {
    /// Marks an element as needing build.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        self.needs_build.insert(id);
    }

    /// Marks an element as needing layout.
    pub fn mark_needs_layout(&mut self, id: ElementId) {
        self.needs_layout.insert(id);
        // Layout changes require repaint (Flutter pattern)
        self.mark_needs_paint(id);
    }

    /// Marks an element as needing paint.
    pub fn mark_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.insert(id);
    }

    /// Marks an element as needing compositing update.
    pub fn mark_needs_compositing(&mut self, id: ElementId) {
        self.needs_compositing.insert(id);
    }

    /// Returns and clears elements needing build.
    pub fn take_needs_build(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_build)
    }

    /// Returns and clears elements needing layout.
    pub fn take_needs_layout(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_layout)
    }

    /// Returns and clears elements needing paint.
    pub fn take_needs_paint(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_paint)
    }

    /// Returns and clears elements needing compositing.
    pub fn take_needs_compositing(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_compositing)
    }

    /// Checks if any element needs build.
    pub fn has_dirty_elements(&self) -> bool {
        !self.needs_build.is_empty()
    }
}

impl Default for TreeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

// ========== Debug ==========
impl std::fmt::Debug for TreeCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeCoordinator")
            .field("root", &self.root)
            .field("elements_count", &self.elements.len())
            .field("views_count", &self.views.len())
            .field("render_objects_count", &self.render_objects.len())
            .field("layers_count", &self.layers.len())
            .field("needs_build", &self.needs_build.len())
            .field("needs_layout", &self.needs_layout.len())
            .field("needs_paint", &self.needs_paint.len())
            .finish()
    }
}
```

### 1.2 Обновить экспорты в mod.rs

**Файл:** `crates/flui_core/src/pipeline/mod.rs`

```rust
// TreeCoordinator export (без дженерика!)
pub use tree_coordinator::TreeCoordinator;
```

### 1.3 Проверка компиляции

```bash
cd crates/flui_core
cargo build
# Исправить все ошибки связанные с TreeCoordinator<E>
```

---

## 🏗️ Фаза 2: Интеграция в PipelineOwner (3-4 часа)

### 2.1 Обновить структуру PipelineOwner

**Файл:** `crates/flui_core/src/pipeline/pipeline_owner.rs`

```rust
use super::TreeCoordinator;  // Add import

pub struct PipelineOwner {
    // NEW: Координатор всех 4 деревьев (заменяет старый tree)
    tree_coord: Arc<RwLock<TreeCoordinator>>,
    
    // Frame coordination (build/layout/paint phases)
    coordinator: FrameCoordinator,
    
    // Root element tracking
    root_mgr: RootManager,
    
    // Deferred rebuilds
    rebuild_queue: RebuildQueue,
    
    // Callback when build scheduled
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
    
    // Frame counter
    frame_counter: u64,
    
    // Optional features
    features: PipelineFeatures,
}

impl PipelineOwner {
    /// Create a new pipeline owner with default configuration
    pub fn new() -> Self {
        let rebuild_queue = RebuildQueue::new();
        
        Self {
            // Create TreeCoordinator (no generic!)
            tree_coord: Arc::new(RwLock::new(TreeCoordinator::new())),
            
            coordinator: FrameCoordinator::new_with_queue(rebuild_queue.clone()),
            root_mgr: RootManager::new(),
            rebuild_queue,
            on_build_scheduled: None,
            frame_counter: 0,
            features: PipelineFeatures::new(),
        }
    }

    // ========== TreeCoordinator Access ==========
    
    /// Get shared reference to tree coordinator
    pub fn tree_coordinator(&self) -> &Arc<RwLock<TreeCoordinator>> {
        &self.tree_coord
    }
    
    // ========== Convenience Accessors (backward compatibility) ==========
    
    /// Access elements through coordinator
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let elements = owner.elements_read();
    /// let element = elements.get(id);
    /// ```
    pub fn elements_read(&self) -> impl std::ops::Deref<Target = ElementTree> + '_ {
        parking_lot::RwLockReadGuard::map(
            self.tree_coord.read(),
            |c| c.elements()
        )
    }
    
    /// Access views through coordinator
    pub fn views_read(&self) -> impl std::ops::Deref<Target = ViewTree> + '_ {
        parking_lot::RwLockReadGuard::map(
            self.tree_coord.read(),
            |c| c.views()
        )
    }
    
    /// Access render objects through coordinator
    pub fn render_objects_read(&self) -> impl std::ops::Deref<Target = RenderTree> + '_ {
        parking_lot::RwLockReadGuard::map(
            self.tree_coord.read(),
            |c| c.render_objects()
        )
    }
}
```

### 2.2 Обновить методы для работы с root

**Файл:** `crates/flui_core/src/pipeline/pipeline_owner.rs`

```rust
impl PipelineOwner {
    /// Set root element
    pub fn set_root(&mut self, element: Element) -> ElementId {
        let mut coord = self.tree_coord.write();
        
        // Insert element into ElementTree
        let root_id = coord.elements_mut().insert(element);
        
        // Set as root in coordinator
        coord.set_root(Some(root_id));
        
        // Update root manager
        self.root_mgr.set_root(Some(root_id));
        
        root_id
    }

    /// Get root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.tree_coord.read().root()
    }

    /// Attach an element to the tree
    pub fn attach(
        &mut self,
        element: Element,
        parent_id: Option<ElementId>,
        slot: Option<Slot>,
    ) -> ElementId {
        let mut coord = self.tree_coord.write();
        
        // Insert element
        let element_id = coord.elements_mut().insert(element);
        
        // Set parent if provided
        if let Some(parent) = parent_id {
            coord.elements_mut()
                .set_parent(element_id, Some(parent))
                .expect("Failed to set parent");
        }
        
        // Mount element
        if let Some(elem) = coord.elements_mut().get_mut(element_id) {
            elem.mount(parent_id, slot, 0);
        }
        
        element_id
    }
}
```

### 2.3 Обновить schedule_build_for

```rust
impl PipelineOwner {
    /// Schedule an element for rebuild
    pub fn schedule_build_for(&mut self, element_id: ElementId, depth: usize) {
        // Mark in coordinator
        self.tree_coord.write().mark_needs_build(element_id);
        
        // Schedule in build pipeline
        self.coordinator.build_mut().schedule(element_id, depth);
        
        // Trigger callback if first dirty element
        if let Some(callback) = &self.on_build_scheduled {
            callback();
        }
    }

    /// Request layout for element
    pub fn request_layout(&mut self, element_id: ElementId) {
        self.tree_coord.write().mark_needs_layout(element_id);
    }

    /// Request paint for element
    pub fn request_paint(&mut self, element_id: ElementId) {
        self.tree_coord.write().mark_needs_paint(element_id);
    }
}
```

### 2.4 Проверка компиляции

```bash
cargo build -p flui_core
# Исправить все ошибки
```

---

## 🔨 Фаза 3: Обновление BuildPipeline (4-5 часов)

### 3.1 Обновить сигнатуру rebuild_dirty

**Файл:** `crates/flui_core/src/pipeline/build_pipeline.rs`

```rust
impl BuildPipeline {
    /// Rebuild all dirty elements
    pub fn rebuild_dirty(
        &mut self,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
    ) -> Result<(), PipelineError> {
        // Get dirty elements from coordinator
        let dirty_ids = {
            let mut coord = coordinator.write();
            coord.take_needs_build()
        };
        
        // Convert to Vec with depths
        let mut dirty_with_depths: Vec<(ElementId, usize)> = {
            let coord = coordinator.read();
            dirty_ids
                .into_iter()
                .filter_map(|id| {
                    coord.elements()
                        .get(id)
                        .map(|elem| (id, elem.depth()))
                })
                .collect()
        };
        
        // Sort by depth (parents first - Flutter pattern)
        dirty_with_depths.sort_by_key(|(_, depth)| *depth);
        
        // Process each element
        let tree_arc = coordinator.clone();
        let dirty_set = self.dirty_set.clone();
        
        for (element_id, depth) in dirty_with_depths {
            self.rebuild_element(element_id, &tree_arc, &dirty_set)?;
        }
        
        Ok(())
    }
}
```

### 3.2 Добавить rebuild_element с TreeCoordinator

```rust
impl BuildPipeline {
    /// Rebuild a single element
    fn rebuild_element(
        &mut self,
        element_id: ElementId,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
        dirty_set: &Arc<RwLock<DirtySet>>,
    ) -> Result<(), PipelineError> {
        // Create BuildContext
        let ctx = PipelineBuildContext::new(
            element_id,
            coordinator.clone(),
            dirty_set.clone(),
        );
        
        // Get element info
        let (view_id, view_mode) = {
            let coord = coordinator.read();
            let element = coord.elements()
                .get(element_id)
                .ok_or_else(|| PipelineError::element_not_found(element_id))?;
            
            let view_id = element.view_id()
                .ok_or_else(|| PipelineError::invalid_element("No ViewId"))?;
            
            (view_id, element.view_mode())
        };
        
        // Call build() on ViewObject
        let child_result = {
            let mut coord = coordinator.write();
            let view_obj = coord.views_mut()
                .get_mut(view_id)
                .ok_or_else(|| PipelineError::view_not_found(view_id))?;
            
            // Call build()
            view_obj.build(&ctx)?
        };
        
        // Process child if returned
        if let Some(child_view) = child_result {
            self.process_child(element_id, child_view, coordinator)?;
        }
        
        // Clear dirty flag
        {
            let mut coord = coordinator.write();
            if let Some(elem) = coord.elements_mut().get_mut(element_id) {
                elem.clear_dirty();
            }
        }
        
        Ok(())
    }
}
```

### 3.3 Добавить process_child helper

```rust
impl BuildPipeline {
    /// Process child view returned from build()
    fn process_child(
        &mut self,
        parent_id: ElementId,
        child_view: Box<dyn ViewObject>,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
    ) -> Result<(), PipelineError> {
        let mut coord = coordinator.write();
        
        // 1. Get child mode
        let child_mode = child_view.mode();
        
        // 2. Insert ViewObject into ViewTree
        let child_view_id = coord.views_mut().insert(child_view);
        
        // 3. Create child Element with ViewId reference
        let child_element = Element::view(Some(child_view_id), child_mode);
        
        // 4. Insert child Element into ElementTree
        let child_id = coord.elements_mut().insert(child_element);
        
        // 5. Set parent-child relationship
        coord.elements_mut()
            .set_parent(child_id, Some(parent_id))
            .map_err(|e| PipelineError::tree_error(e.to_string()))?;
        
        // 6. Mark child for build
        coord.mark_needs_build(child_id);
        
        Ok(())
    }
}
```

### 3.4 Обновить can_reuse с ViewTree доступом

```rust
impl BuildPipeline {
    /// Check if old element can be reused with new view
    fn can_reuse(
        old_element: &Element,
        new_view: &dyn ViewObject,
        view_tree: &ViewTree,
    ) -> bool {
        // 1. Check ViewMode
        if old_element.view_mode() != new_view.mode() {
            return false;
        }
        
        // 2. Check Key matching
        match (old_element.key(), new_view.key()) {
            (Some(old_key), Some(new_key)) => {
                if old_key != new_key {
                    return false;
                }
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        
        // 3. Check TypeId through ViewTree
        if let Some(old_view_id) = old_element.view_id() {
            if let Some(old_view) = view_tree.get(old_view_id) {
                let old_type = old_view.as_any().type_id();
                let new_type = new_view.as_any().type_id();
                
                if old_type != new_type {
                    return false;
                }
            }
        }
        
        true
    }
}
```

### 3.5 Обновить update_element

```rust
impl BuildPipeline {
    /// Update existing element with new view (in-place reuse)
    fn update_element(
        element_id: ElementId,
        new_view: Box<dyn ViewObject>,
        coordinator: &mut TreeCoordinator,
        ctx: &PipelineBuildContext,
    ) {
        // Get old ViewId
        let old_view_id = {
            let element = match coordinator.elements().get(element_id) {
                Some(e) => e,
                None => return,
            };
            
            match element.view_id() {
                Some(id) => id,
                None => return,
            }
        };
        
        // Call did_update() lifecycle hook
        {
            let old_view = coordinator.views().get(old_view_id).unwrap();
            new_view.did_update(old_view.as_any(), ctx);
        }
        
        // Replace old view with new view in ViewTree
        coordinator.views_mut().replace(old_view_id, new_view);
        
        // Mark dirty to trigger rebuild
        coordinator.mark_needs_build(element_id);
    }
}
```

### 3.6 Проверка компиляции

```bash
cargo build -p flui_core
cargo test -p flui_core --lib build_pipeline
```

---

## 🔌 Фаза 4: Обновление PipelineBuildContext (2-3 часа)

### 4.1 Обновить структуру PipelineBuildContext

**Файл:** `crates/flui-pipeline/src/context.rs`

```rust
use flui_core::pipeline::TreeCoordinator;  // Add import

/// Build context for pipeline operations
pub struct PipelineBuildContext {
    /// ID of element being built
    element_id: ElementId,
    
    /// Coordinator with all four trees
    coordinator: Arc<RwLock<TreeCoordinator>>,
    
    /// Dirty set for scheduling rebuilds
    dirty_set: Arc<RwLock<DirtySet>>,
    
    /// Cached depth (computed on first access)
    depth_cache: Cell<Option<usize>>,
}

impl PipelineBuildContext {
    /// Create new build context
    pub fn new(
        element_id: ElementId,
        coordinator: Arc<RwLock<TreeCoordinator>>,
        dirty_set: Arc<RwLock<DirtySet>>,
    ) -> Self {
        Self {
            element_id,
            coordinator,
            dirty_set,
            depth_cache: Cell::new(None),
        }
    }
    
    /// Get coordinator reference
    pub fn coordinator(&self) -> &Arc<RwLock<TreeCoordinator>> {
        &self.coordinator
    }
}
```

### 4.2 Реализовать BuildContext trait

```rust
impl BuildContext for PipelineBuildContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }
    
    fn depth(&self) -> usize {
        // Use cached value if available
        if let Some(depth) = self.depth_cache.get() {
            return depth;
        }
        
        // Compute depth
        let coord = self.coordinator.read();
        let depth = coord.elements()
            .get(self.element_id)
            .map(|e| e.depth())
            .unwrap_or(0);
        
        // Cache it
        self.depth_cache.set(Some(depth));
        depth
    }
    
    fn mark_dirty(&self) {
        let mut dirty = self.dirty_set.write();
        dirty.mark_dirty(self.element_id);
    }
    
    fn schedule_rebuild(&self, element_id: ElementId) {
        let mut dirty = self.dirty_set.write();
        dirty.mark_dirty(element_id);
        
        // Also mark in coordinator
        let mut coord = self.coordinator.write();
        coord.mark_needs_build(element_id);
    }
    
    fn for_child(&self, child_id: ElementId) -> Self {
        Self {
            element_id: child_id,
            coordinator: self.coordinator.clone(),
            dirty_set: self.dirty_set.clone(),
            depth_cache: Cell::new(None),
        }
    }
    
    fn visit_ancestors(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let coord = self.coordinator.read();
        let mut current_id = coord.elements()
            .get(self.element_id)
            .and_then(|e| e.parent());
        
        while let Some(id) = current_id {
            if !visitor(id) {
                break;
            }
            current_id = coord.elements()
                .get(id)
                .and_then(|e| e.parent());
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}
```

### 4.3 Добавить helper методы

```rust
impl PipelineBuildContext {
    /// Access ViewObject by ID
    pub fn get_view<V: ViewObject + 'static>(&self, view_id: ViewId) -> Option<&V> {
        let coord = self.coordinator.read();
        coord.views()
            .get(view_id)
            .and_then(|v| v.as_any().downcast_ref())
    }
    
    /// Access current element's ViewObject
    pub fn current_view<V: ViewObject + 'static>(&self) -> Option<&V> {
        let coord = self.coordinator.read();
        let element = coord.elements().get(self.element_id)?;
        let view_id = element.view_id()?;
        self.get_view(view_id)
    }
    
    /// Access RenderObject by ID
    pub fn get_render<R: RenderObject + 'static>(&self, render_id: RenderId) -> Option<&R> {
        let coord = self.coordinator.read();
        coord.render_objects()
            .get(render_id)
            .and_then(|r| r.as_any().downcast_ref())
    }
}
```

### 4.4 Проверка компиляции

```bash
cargo build -p flui-pipeline
cargo build -p flui_core
```

---

## 🎨 Фаза 5: Обновление FrameCoordinator (1-2 часа)

### 5.1 Обновить build_frame

**Файл:** `crates/flui_core/src/pipeline/frame_coordinator.rs`

```rust
impl FrameCoordinator {
    /// Build complete frame (all three phases)
    pub fn build_frame(
        &mut self,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
        root_id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<BoxedLayer, PipelineError> {
        // Phase 1: Build
        self.flush_build(coordinator)?;
        
        // Phase 2: Layout
        let size = self.flush_layout(coordinator, root_id, constraints)?;
        
        // Phase 3: Paint
        let layer = self.flush_paint(coordinator, root_id)?;
        
        Ok(layer)
    }
    
    /// Flush build phase
    pub fn flush_build(
        &mut self,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
    ) -> Result<(), PipelineError> {
        self.build.rebuild_dirty(coordinator)
    }
    
    /// Flush layout phase
    pub fn flush_layout(
        &mut self,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
        root_id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, PipelineError> {
        self.layout.layout_tree(coordinator, root_id, constraints)
    }
    
    /// Flush paint phase
    pub fn flush_paint(
        &mut self,
        coordinator: &Arc<RwLock<TreeCoordinator>>,
        root_id: ElementId,
    ) -> Result<BoxedLayer, PipelineError> {
        self.paint.paint_tree(coordinator, root_id)
    }
}
```

### 5.2 Проверка компиляции

```bash
cargo build -p flui_core
```

---

## 🧹 Фаза 6: Cleanup deprecated кода (2-3 часа)

### 6.1 Удалить deprecated методы из Element

**Файл:** `crates/flui-element/src/element/element.rs`

Найти и удалить все методы помеченные `#[deprecated]`:

```rust
// УДАЛИТЬ все методы с #[deprecated]:
// - view_object()
// - view_object_mut()
// - has_view_object()
// - view_object_any()
// - take_view_object()
// - set_view_object()
// - render_object()
// - render_object_mut()
// - render_state()
// - render_state_mut()
```

### 6.2 Удалить deprecated IntoElement impls

**Файл:** `crates/flui-element/src/into_element.rs`

```rust
// УДАЛИТЬ deprecated impl'ы:
// - impl IntoElement for Box<dyn ViewObject>
// - impl IntoElement for BoxRenderWrapper<A>
// - impl IntoElement for SliverRenderWrapper<A>
```

### 6.3 Обновить migration guide

**Файл:** `docs/MIGRATION_FOUR_TREE.md` (создать новый)

```markdown
# Migration Guide: Four-Tree Architecture

## Overview

FLUI has migrated to a four-tree architecture where ViewObjects, Elements, 
RenderObjects, and Layers are stored in separate trees.

## Breaking Changes

### 1. Element no longer stores ViewObject directly

**Before:**
```rust
let element = Element::new(my_view_object);
if let Some(view) = element.view_object() {
    // ...
}
```

**After:**
```rust
// Insert ViewObject into ViewTree first
let view_id = coordinator.views_mut().insert(my_view_object);

// Create Element with ViewId reference
let element = Element::view(Some(view_id), ViewMode::Stateless);

// Access ViewObject through coordinator
if let Some(view) = coordinator.views().get(view_id) {
    // ...
}
```

### 2. IntoElement deprecated for ViewObject

**Before:**
```rust
let element = my_view_object.into_element();
```

**After:**
```rust
// Manual two-step process:
let view_id = view_tree.insert(my_view_object);
let element = Element::view(Some(view_id), mode);
```

### 3. BuildPipeline requires TreeCoordinator

**Before:**
```rust
pipeline.rebuild_dirty(&mut element_tree)?;
```

**After:**
```rust
pipeline.rebuild_dirty(&tree_coordinator)?;
```

## Benefits

- Clear separation of concerns
- Better performance (ID-based access)
- Type safety
- Flutter-aligned architecture
```

### 6.4 Проверка

```bash
# Найти все использования deprecated методов
rg "#\[deprecated\]" crates/
rg "\.view_object\(\)" crates/
rg "\.into_element\(\)" crates/ --type rust
```

---

## ✅ Фаза 7: Тесты (4-6 часов)

### 7.1 Создать unit тесты для TreeCoordinator

**Файл:** `crates/flui_core/src/pipeline/tree_coordinator.rs` (tests module)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tree_coordinator_creation() {
        let coord = TreeCoordinator::new();
        assert_eq!(coord.elements().len(), 0);
        assert_eq!(coord.views().len(), 0);
    }
    
    #[test]
    fn test_dirty_tracking() {
        let mut coord = TreeCoordinator::new();
        let id = ElementId::new(1).unwrap();
        
        coord.mark_needs_build(id);
        assert!(coord.has_dirty_elements());
        
        let dirty = coord.take_needs_build();
        assert!(dirty.contains(&id));
    }
    
    #[test]
    fn test_mark_needs_layout_marks_paint() {
        let mut coord = TreeCoordinator::new();
        let id = ElementId::new(1).unwrap();
        
        coord.mark_needs_layout(id);
        
        let needs_paint = coord.take_needs_paint();
        assert!(needs_paint.contains(&id));
    }
}
```

### 7.2 Интеграционные тесты

**Файл:** `crates/flui_core/tests/integration_four_tree.rs` (новый)

```rust
//! Integration tests for four-tree architecture

use flui_core::pipeline::PipelineOwner;

#[test]
fn test_full_frame_with_coordinator() {
    let mut owner = PipelineOwner::new();
    
    // Create and insert root view
    let root_id = {
        let mut coord = owner.tree_coordinator().write();
        let view_id = coord.views_mut().insert(Box::new(TestView));
        let element = Element::view(Some(view_id), ViewMode::Stateful);
        let element_id = coord.elements_mut().insert(element);
        coord.set_root(Some(element_id));
        element_id
    };
    
    // Build frame
    let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    let result = owner.build_frame(constraints);
    
    assert!(result.is_ok());
}
```

### 7.3 Performance benchmarks

**Файл:** `crates/flui_core/benches/tree_coordinator.rs` (новый)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_core::pipeline::TreeCoordinator;

fn bench_coordinator_access(c: &mut Criterion) {
    let coord = TreeCoordinator::new();
    
    c.bench_function("coordinator_elements_access", |b| {
        b.iter(|| black_box(coord.elements().len()));
    });
}

criterion_group!(benches, bench_coordinator_access);
criterion_main!(benches);
```

### 7.4 Запуск тестов

```bash
# Unit tests
cargo test -p flui_core

# Integration tests
cargo test -p flui_core --test integration_four_tree

# All tests
cargo test --all

# Benchmarks
cargo bench -p flui_core
```

---

## 📚 Фаза 8: Документация (2-3 часа)

### 8.1 Обновить README flui_core

**Файл:** `crates/flui_core/README.md`

Добавить секцию:

```markdown
## Four-Tree Architecture

FLUI uses a four-tree architecture:

```text
ViewTree          ElementTree        RenderTree         LayerTree
(ViewObjects)     (Elements)         (RenderObjects)    (Layers)
    ↓                  ↓                  ↓                ↓
Immutable         Lifecycle          Layout/Paint       Compositing
```

### TreeCoordinator

```rust
let coordinator = TreeCoordinator::new();

// Insert ViewObject
let view_id = coordinator.views_mut().insert(my_view);

// Create Element
let element = Element::view(Some(view_id), ViewMode::Stateful);
let element_id = coordinator.elements_mut().insert(element);
```
```

### 8.2 Создать architecture doc

**Файл:** `docs/architecture/FOUR_TREE_ARCHITECTURE.md` (новый)

```markdown
# FLUI Four-Tree Architecture

## Overview

FLUI separates UI concerns into four distinct trees.

## The Four Trees

### 1. ViewTree
- Stores immutable ViewObjects
- Like Flutter's Widget tree

### 2. ElementTree  
- Manages element lifecycle
- Persistent across rebuilds

### 3. RenderTree
- Layout and paint logic
- Like Flutter's RenderObject tree

### 4. LayerTree
- GPU compositing
- Explicit compositor layers

## Comparison with Flutter

| Aspect | Flutter | FLUI |
|--------|---------|------|
| Widget storage | Direct reference | ViewTree + ViewId |
| Render storage | Direct reference | RenderTree + RenderId |
| Coordinator | BuildOwner + PipelineOwner | TreeCoordinator |
```

### 8.3 Создать примеры

**Файл:** `examples/four_tree_usage.rs` (новый)

```rust
//! Example demonstrating four-tree architecture

use flui_core::pipeline::PipelineOwner;

fn main() {
    let mut owner = PipelineOwner::new();
    
    // Create view
    let my_view = MyWidget::new("Hello");
    
    // Insert into ViewTree
    let view_id = {
        let mut coord = owner.tree_coordinator().write();
        coord.views_mut().insert(Box::new(my_view))
    };
    
    println!("Created ViewId: {:?}", view_id);
}
```

---

## 🎯 Фаза 9: Финальная проверка (1-2 часа)

### 9.1 Проверочный скрипт

**Файл:** `run_checks.sh` (новый)

```bash
#!/bin/bash

echo "=== Compilation ==="
cargo build --all || exit 1

echo "=== Tests ==="
cargo test --all || exit 1

echo "=== Clippy ==="
cargo clippy --all -- -D warnings || exit 1

echo "=== Format ==="
cargo fmt --all -- --check || exit 1

echo "=== Documentation ==="
cargo doc --all --no-deps || exit 1

echo "✅ All checks passed!"
```

### 9.2 Code review checklist

```markdown
## Code Review Checklist

### Architecture
- [ ] TreeCoordinator has no generic
- [ ] All four trees are concrete types
- [ ] PipelineOwner uses TreeCoordinator
- [ ] No circular dependencies

### API Design
- [ ] Clear accessor methods
- [ ] Good error messages
- [ ] Consistent naming

### Performance
- [ ] No performance regressions
- [ ] Efficient ID lookups

### Documentation
- [ ] All public APIs documented
- [ ] Migration guide complete
- [ ] Examples work

### Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Edge cases covered

### Code Quality
- [ ] No clippy warnings
- [ ] Formatted correctly
- [ ] Clean commit history
```

---

## 📊 Summary

### Временные оценки

| Фаза | Описание | Время |
|------|----------|-------|
| 0 | Подготовка | 1-2 ч |
| 1 | TreeCoordinator refactor | 2-3 ч |
| 2 | PipelineOwner integration | 3-4 ч |
| 3 | BuildPipeline refactor | 4-5 ч |
| 4 | PipelineBuildContext | 2-3 ч |
| 5 | FrameCoordinator | 1-2 ч |
| 6 | Cleanup deprecated | 2-3 ч |
| 7 | Tests | 4-6 ч |
| 8 | Documentation | 2-3 ч |
| 9 | Final checks | 1-2 ч |
| **TOTAL** | | **22-33 ч** |

### Ключевые изменения

1. ✅ TreeCoordinator без дженерика
2. ✅ PipelineOwner с TreeCoordinator
3. ✅ BuildPipeline через ViewTree
4. ✅ PipelineBuildContext с доступом ко всем деревьям
5. ✅ Reconciliation через ViewTree
6. ✅ Удален deprecated код
7. ✅ Полная документация

### Риски и митигация

| Риск | Вероятность | Митигация |
|------|-------------|-----------|
| Breaking changes | Высокая | Migration guide |
| Performance regression | Средняя | Benchmarks |
| Compilation errors | Высокая | Пошаговый подход |
| Test failures | Средняя | Обновить тесты |

---

## 🚀 Начало работы

```bash
# 1. Создать ветку
git checkout -b refactor/integrate-tree-coordinator

# 2. Начать с Фазы 1
cd crates/flui_core/src/pipeline
# Редактировать tree_coordinator.rs

# 3. Проверять после каждой фазы
cargo build -p flui_core
cargo test -p flui_core
```

---

**Готов начать? Скажи с какой фазы начинаем!** 🚀
