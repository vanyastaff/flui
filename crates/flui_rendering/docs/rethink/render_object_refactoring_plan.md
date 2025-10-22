# Глубокий рефакторинг RenderObject архитектуры

## Анализ текущей ситуации

### Что есть сейчас (хорошее):
- ✅ `impl_cached_layout!` макрос - эффективное кэширование
- ✅ `RenderFlags` - битфлаги для экономии памяти (1 byte вместо 2-4)
- ✅ `ElementId` система для кэш-инвалидации
- ✅ Разделение на `DynRenderObject` (object-safe) и `RenderObject` (с associated types)

### Проблемы (требующие решения):

#### 1. Massive Field Duplication
Каждый RenderObject повторяет ~5 полей:
```rust
pub struct RenderOpacity {
    element_id: Option<ElementId>,           // 16 bytes
    child: Option<Box<dyn DynRenderObject>>, // 16 bytes
    size: Size,                               // 8 bytes
    constraints: Option<BoxConstraints>,      // 40 bytes
    flags: RenderFlags,                       // 1 byte
    // + специфичные поля (opacity, padding, transform, etc.)
}
```

**Цена:** ~80 bytes × 50+ типов = минимум 4KB только на повторяющиеся поля

#### 2. Method Duplication
Каждый RenderObject реализует ~10-15 одинаковых методов:
```rust
// Одинаковая логика в 50+ местах:
pub fn element_id(&self) -> Option<ElementId> { self.element_id }
pub fn set_element_id(&mut self, id: Option<ElementId>) { self.element_id = id; }
pub fn child(&self) -> Option<&dyn DynRenderObject> { self.child.as_deref() }
pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
    self.child = child;
    self.mark_needs_layout();
}
// ... еще 10 методов
```

**Цена:** ~200 lines × 50+ типов = 10,000+ lines boilerplate кода

#### 3. DynRenderObject Implementation Duplication
Повторяющиеся паттерны в impl блоках:
```rust
impl DynRenderObject for RenderXXX {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self, constraints, {
            // Специфичная логика
        })
    }
    
    fn needs_layout(&self) -> bool { self.flags.needs_layout() }
    fn mark_needs_layout(&mut self) { self.flags.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.flags.needs_paint() }
    fn mark_needs_paint(&mut self) { self.flags.mark_needs_paint() }
    fn size(&self) -> Size { self.size }
    fn constraints(&self) -> Option<BoxConstraints> { self.constraints }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child { visitor(&**child); }
    }
    
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child { visitor(&mut **child); }
    }
    
    // ... еще методы
}
```

**Цена:** ~150 lines × 50+ типов = 7,500+ lines дублированного кода

#### 4. Несогласованность между старым и новым кодом
- Некоторые используют `RenderFlags`, другие отдельные `needs_layout_flag`
- Некоторые используют `element_id`, другие нет
- Некоторые используют cached layout, другие нет

---

## Архитектурное решение: Compositional Building Blocks

### Философия
Вместо inheritance (которого нет в Rust), используем **composition + generics + procedural macros**.

### Ключевые компоненты:

## 1. Core Building Block: `SingleChildRenderCore`

```rust
/// Ядро single-child render object с всем базовым функционалом
/// 
/// Это НЕ trait, а struct который можно встроить в любой RenderObject
#[derive(Debug)]
pub struct SingleChildRenderCore {
    /// Element ID для кэширования
    pub element_id: Option<ElementId>,
    
    /// Child render object
    pub child: Option<Box<dyn DynRenderObject>>,
    
    /// Текущий размер после layout
    pub size: Size,
    
    /// Текущие constraints
    pub constraints: Option<BoxConstraints>,
    
    /// Битфлаги состояния
    pub flags: RenderFlags,
}

impl SingleChildRenderCore {
    /// Создать новое ядро
    pub const fn new() -> Self {
        Self {
            element_id: None,
            child: None,
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    /// Создать с ElementId (для кэширования)
    pub const fn with_element_id(element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            child: None,
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    // ===== Element ID Management =====
    
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }
    
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
    
    // ===== Child Management =====
    
    #[inline]
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }
    
    #[inline]
    pub fn child_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
        self.child.as_deref_mut()
    }
    
    #[inline]
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.flags.mark_needs_layout();
    }
    
    #[inline]
    pub fn take_child(&mut self) -> Option<Box<dyn DynRenderObject>> {
        let child = self.child.take();
        if child.is_some() {
            self.flags.mark_needs_layout();
        }
        child
    }
    
    // ===== Layout State =====
    
    #[inline]
    pub fn size(&self) -> Size {
        self.size
    }
    
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }
    
    #[inline]
    pub fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }
    
    #[inline]
    pub fn set_constraints(&mut self, constraints: BoxConstraints) {
        self.constraints = Some(constraints);
    }
    
    // ===== Flags Management =====
    
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }
    
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }
    
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.flags.mark_needs_layout();
    }
    
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.flags.mark_needs_paint();
    }
    
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.clear_needs_layout();
    }
    
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.clear_needs_paint();
    }
    
    // ===== Common Layout Patterns =====
    
    /// Passthrough layout - просто передать constraints child'у
    pub fn layout_passthrough(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            self.size = child.layout(constraints);
        } else {
            self.size = constraints.smallest();
        }
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    /// Common visitor pattern for single child
    #[inline]
    pub fn visit_child(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }
    
    #[inline]
    pub fn visit_child_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
    
    /// Common hit test pattern - delegate to child
    #[inline]
    pub fn hit_test_child(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }
    
    /// Common paint pattern - paint child at offset
    #[inline]
    pub fn paint_child(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }
}

impl Default for SingleChildRenderCore {
    fn default() -> Self {
        Self::new()
    }
}
```

### Преимущества `SingleChildRenderCore`:
1. **Одно место для всех базовых полей** - изменение в одном месте влияет на все
2. **Zero-cost** - все методы #[inline], compiler их оптимизирует
3. **Переиспользуемые паттерны** - passthrough layout, visit, hit_test, paint
4. **Типобезопасность** - все методы работают с правильными типами

---

## 2. Derive Macro для делегирования

```rust
/// Derive macro для автоматического делегирования методов к core
/// 
/// # Example
/// 
/// ```rust,ignore
/// #[derive(RenderObjectCore)]
/// #[render_core(field = "core")]  // указываем поле с SingleChildRenderCore
/// pub struct RenderOpacity {
///     core: SingleChildRenderCore,
///     opacity: f32,
/// }
/// 
/// // Macro генерирует:
/// impl RenderOpacity {
///     pub fn element_id(&self) -> Option<ElementId> { self.core.element_id() }
///     pub fn set_element_id(&mut self, id: Option<ElementId>) { self.core.set_element_id(id) }
///     pub fn child(&self) -> Option<&dyn DynRenderObject> { self.core.child() }
///     // ... все остальные методы
/// }
/// ```
#[proc_macro_derive(RenderObjectCore, attributes(render_core))]
pub fn derive_render_object_core(input: TokenStream) -> TokenStream {
    // Парсим input
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    
    // Получаем имя поля из атрибута
    let field_name = /* parse from attributes */;
    
    let expanded = quote! {
        impl #name {
            // Element ID methods
            #[inline]
            pub fn element_id(&self) -> Option<ElementId> {
                self.#field_name.element_id()
            }
            
            #[inline]
            pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
                self.#field_name.set_element_id(element_id)
            }
            
            // Child methods
            #[inline]
            pub fn child(&self) -> Option<&dyn DynRenderObject> {
                self.#field_name.child()
            }
            
            #[inline]
            pub fn child_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
                self.#field_name.child_mut()
            }
            
            #[inline]
            pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
                self.#field_name.set_child(child)
            }
            
            #[inline]
            pub fn take_child(&mut self) -> Option<Box<dyn DynRenderObject>> {
                self.#field_name.take_child()
            }
            
            // Layout state methods  
            #[inline]
            pub fn size(&self) -> Size {
                self.#field_name.size()
            }
            
            #[inline]
            pub fn constraints(&self) -> Option<BoxConstraints> {
                self.#field_name.constraints()
            }
            
            // Flag methods
            #[inline]
            pub fn mark_needs_layout(&mut self) {
                self.#field_name.mark_needs_layout()
            }
            
            #[inline]
            pub fn mark_needs_paint(&mut self) {
                self.#field_name.mark_needs_paint()
            }
            
            #[inline]
            pub fn needs_layout(&self) -> bool {
                self.#field_name.needs_layout()
            }
            
            #[inline]
            pub fn needs_paint(&self) -> bool {
                self.#field_name.needs_paint()
            }
        }
    };
    
    TokenStream::from(expanded)
}
```

---

## 3. Auto-implement DynRenderObject trait

```rust
/// Macro для автоматической генерации impl DynRenderObject
/// 
/// Генерирует все стандартные методы, оставляя только layout/paint/hit_test
/// 
/// # Example
/// 
/// ```rust,ignore
/// #[impl_dyn_render_object(core_field = "core")]
/// impl DynRenderObject for RenderOpacity {
///     // Только специфичные методы:
///     
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         impl_cached_layout!(self.core, constraints, {
///             self.core.layout_passthrough(constraints)
///         })
///     }
///     
///     fn paint(&self, painter: &egui::Painter, offset: Offset) {
///         if !self.is_transparent() {
///             self.core.paint_child(painter, offset);
///         }
///     }
///     
///     fn hit_test_self(&self, _position: Offset) -> bool {
///         !self.is_transparent()
///     }
/// }
/// 
/// // Macro auto-generates:
/// // - size(), constraints()
/// // - needs_layout(), mark_needs_layout()  
/// // - needs_paint(), mark_needs_paint()
/// // - visit_children(), visit_children_mut()
/// // - hit_test() (delegating to hit_test_self + hit_test_children)
/// // - hit_test_children() (delegating to core)
/// ```
#[proc_macro_attribute]
pub fn impl_dyn_render_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    let impl_block = parse_macro_input!(item as ItemImpl);
    let core_field = /* parse from attr */;
    
    // Check which methods are already implemented
    let has_layout = /* check if layout() exists */;
    let has_paint = /* check if paint() exists */;
    let has_hit_test_self = /* check if hit_test_self() exists */;
    
    let expanded = quote! {
        #impl_block
        
        // Auto-generate missing standard methods:
        
        #[inline]
        fn size(&self) -> Size {
            self.#core_field.size()
        }
        
        #[inline]
        fn constraints(&self) -> Option<BoxConstraints> {
            self.#core_field.constraints()
        }
        
        #[inline]
        fn needs_layout(&self) -> bool {
            self.#core_field.needs_layout()
        }
        
        #[inline]
        fn mark_needs_layout(&mut self) {
            self.#core_field.mark_needs_layout()
        }
        
        #[inline]
        fn needs_paint(&self) -> bool {
            self.#core_field.needs_paint()
        }
        
        #[inline]
        fn mark_needs_paint(&mut self) {
            self.#core_field.mark_needs_paint()
        }
        
        #[inline]
        fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
            self.#core_field.visit_child(visitor)
        }
        
        #[inline]
        fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
            self.#core_field.visit_child_mut(visitor)
        }
        
        #[inline]
        fn hit_test_children(
            &self,
            result: &mut HitTestResult,
            position: Offset,
        ) -> bool {
            self.#core_field.hit_test_child(result, position)
        }
        
        // hit_test() with default bounds checking + self + children
        fn hit_test(
            &self,
            result: &mut HitTestResult,
            position: Offset,
        ) -> bool {
            // Bounds check
            if position.dx < 0.0
                || position.dx >= self.size().width
                || position.dy < 0.0
                || position.dy >= self.size().height
            {
                return false;
            }
            
            // Check children first (front-to-back)
            let hit_child = self.hit_test_children(result, position);
            
            // Then check self
            let hit_self = self.hit_test_self(position);
            
            if hit_child || hit_self {
                result.add(HitTestEntry::new(position, self.size()));
                return true;
            }
            
            false
        }
    };
    
    TokenStream::from(expanded)
}
```

---

## 4. Пример использования: RenderOpacity (ДО и ПОСЛЕ)

### БЫЛО (старый код):

```rust
#[derive(Debug)]
pub struct RenderOpacity {
    element_id: Option<ElementId>,
    opacity: f32,
    child: Option<Box<dyn DynRenderObject>>,
    size: Size,
    constraints: Option<BoxConstraints>,
    flags: RenderFlags,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        assert!((0.0..=1.0).contains(&opacity));
        Self {
            element_id: None,
            opacity,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    pub fn element_id(&self) -> Option<ElementId> { self.element_id }
    pub fn set_element_id(&mut self, id: Option<ElementId>) { self.element_id = id; }
    pub fn child(&self) -> Option<&dyn DynRenderObject> { self.child.as_deref() }
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }
    // ... еще 10+ методов
}

impl DynRenderObject for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self, constraints, {
            if let Some(child) = &mut self.child {
                child.layout(constraints)
            } else {
                constraints.smallest()
            }
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            if !self.is_transparent() {
                child.paint(painter, offset);
            }
        }
    }
    
    fn needs_layout(&self) -> bool { self.flags.needs_layout() }
    fn mark_needs_layout(&mut self) { self.flags.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.flags.needs_paint() }
    fn mark_needs_paint(&mut self) { self.flags.mark_needs_paint() }
    fn size(&self) -> Size { self.size }
    fn constraints(&self) -> Option<BoxConstraints> { self.constraints }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }
    
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
    
    fn hit_test_self(&self, _position: Offset) -> bool {
        !self.is_transparent()
    }
    
    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if self.is_transparent() { return false; }
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }
}

// 150+ lines кода
```

### СТАЛО (новый код):

```rust
#[derive(Debug, RenderObjectCore)]
#[render_core(field = "core")]
pub struct RenderOpacity {
    core: SingleChildRenderCore,
    opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        assert!((0.0..=1.0).contains(&opacity));
        Self {
            core: SingleChildRenderCore::new(),
            opacity,
        }
    }
    
    pub fn with_element_id(element_id: ElementId, opacity: f32) -> Self {
        assert!((0.0..=1.0).contains(&opacity));
        Self {
            core: SingleChildRenderCore::with_element_id(element_id),
            opacity,
        }
    }
    
    // Только специфичные методы:
    
    pub fn set_opacity(&mut self, opacity: f32) {
        assert!((0.0..=1.0).contains(&opacity));
        if (self.opacity - opacity).abs() > f32::EPSILON {
            self.opacity = opacity;
            self.core.mark_needs_paint();
        }
    }
    
    pub fn opacity(&self) -> f32 {
        self.opacity
    }
    
    pub fn is_transparent(&self) -> bool {
        self.opacity < f32::EPSILON
    }
    
    pub fn is_opaque(&self) -> bool {
        (self.opacity - 1.0).abs() < f32::EPSILON
    }
}

#[impl_dyn_render_object(core_field = "core")]
impl DynRenderObject for RenderOpacity {
    // Только специфичные методы, всё остальное auto-generated:
    
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self.core, constraints, {
            self.core.layout_passthrough(constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if !self.is_transparent() {
            self.core.paint_child(painter, offset);
        }
    }
    
    fn hit_test_self(&self, _position: Offset) -> bool {
        !self.is_transparent()
    }
}

// 70 lines кода (вместо 150+) - экономия 50%
```

---

## 5. Multi-Child Support: `MultiChildRenderCore`

```rust
/// Ядро для multi-child render objects (Flex, Stack, etc.)
#[derive(Debug)]
pub struct MultiChildRenderCore<P: ParentData> {
    pub element_id: Option<ElementId>,
    pub children: Vec<ChildEntry<P>>,
    pub size: Size,
    pub constraints: Option<BoxConstraints>,
    pub flags: RenderFlags,
}

#[derive(Debug)]
pub struct ChildEntry<P: ParentData> {
    pub render_object: Box<dyn DynRenderObject>,
    pub parent_data: P,
    pub offset: Offset,
}

impl<P: ParentData> MultiChildRenderCore<P> {
    pub const fn new() -> Self {
        Self {
            element_id: None,
            children: Vec::new(),
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
    
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>, parent_data: P) {
        self.children.push(ChildEntry {
            render_object: child,
            parent_data,
            offset: Offset::ZERO,
        });
        self.flags.mark_needs_layout();
    }
    
    pub fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        for child in &self.children {
            visitor(&*child.render_object);
        }
    }
    
    pub fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        for child in &mut self.children {
            visitor(&mut *child.render_object);
        }
    }
    
    // ... остальные методы
}
```

### Пример: RenderFlex с `MultiChildRenderCore`

```rust
#[derive(Debug, RenderObjectCore)]
#[render_core(field = "core", multi_child)]
pub struct RenderFlex {
    core: MultiChildRenderCore<FlexParentData>,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            core: MultiChildRenderCore::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
        }
    }
}

#[impl_dyn_render_object(core_field = "core", multi_child)]
impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self.core, constraints, self.core.child_count(), {
            self.perform_flex_layout(constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        for child in &self.core.children {
            let child_offset = offset + child.offset;
            child.render_object.paint(painter, child_offset);
        }
    }
}

// 50 lines вместо 200+
```

---

## 6. Layout Strategy Traits (Advanced)

Для еще большей переиспользуемости:

```rust
/// Trait для различных стратегий layout
pub trait LayoutStrategy: Send + Sync {
    /// Compute layout и вернуть размер
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        constraints: BoxConstraints,
    ) -> Size;
}

/// Passthrough layout стратегия
#[derive(Debug, Default)]
pub struct PassthroughLayout;

impl LayoutStrategy for PassthroughLayout {
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        constraints: BoxConstraints,
    ) -> Size {
        core.layout_passthrough(constraints)
    }
}

/// Modified constraints layout
pub struct ModifiedLayout<F>
where
    F: Fn(BoxConstraints) -> BoxConstraints + Send + Sync,
{
    modifier: F,
}

impl<F> LayoutStrategy for ModifiedLayout<F>
where
    F: Fn(BoxConstraints) -> BoxConstraints + Send + Sync,
{
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        constraints: BoxConstraints,
    ) -> Size {
        let modified = (self.modifier)(constraints);
        if let Some(child) = core.child_mut() {
            let child_size = child.layout(modified);
            // Post-processing можно добавить
            child_size
        } else {
            constraints.smallest()
        }
    }
}

/// Usage:
pub struct RenderPadding {
    core: SingleChildRenderCore,
    padding: EdgeInsets,
    layout_strategy: Box<dyn LayoutStrategy>,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        let padding_clone = padding;
        Self {
            core: SingleChildRenderCore::new(),
            padding,
            layout_strategy: Box::new(ModifiedLayout {
                modifier: move |c| c.deflate(padding_clone),
            }),
        }
    }
}
```

---

## Метрики улучшения

### Сокращение кода:
- **Поля:** ~80 bytes × 50 типов = **4KB дублирования → 0KB** (использование core)
- **Методы:** ~200 lines × 50 типов = **10,000 lines → 0 lines** (derive macro)
- **DynRenderObject impl:** ~150 lines × 50 типов = **7,500 lines → ~50 lines** (auto-impl macro)
- **Итого:** ~17,500 lines → ~2,500 lines = **85% сокращение boilerplate**

### Производительность:
- **Zero-cost:** Все методы #[inline], compiler оптимизирует
- **Кэширование:** Сохраняется через `impl_cached_layout!`
- **Memory:** Та же память (composition не добавляет overhead)

### Maintainability:
- **DRY:** Изменение в `SingleChildRenderCore` влияет на все типы
- **Consistency:** Все RenderObject'ы используют один паттерн
- **Extensibility:** Легко добавлять новые стратегии и типы

### Type Safety:
- **Compile-time guarantees:** Все проверяется на этапе компиляции
- **No runtime cost:** Никаких dynamic dispatch где не нужно
- **Clear APIs:** Явные интерфейсы через traits

---

## План миграции

### Phase 1: Foundation (неделя 1)
1. Создать `SingleChildRenderCore` struct
2. Написать derive macro `RenderObjectCore`
3. Написать attribute macro `impl_dyn_render_object`
4. Тесты на примере 2-3 простых RenderObject'ов

### Phase 2: Single-Child Migration (неделя 2-3)
1. Мигрировать simple RenderObject'ы:
   - RenderOpacity
   - RenderClipRect, RenderClipRRect
   - RenderPadding
   - RenderTransform
2. Обеспечить все tests passing
3. Benchmark производительности

### Phase 3: Multi-Child Support (неделя 4)
1. Создать `MultiChildRenderCore<P>`
2. Расширить macros для multi-child
3. Мигрировать:
   - RenderFlex
   - RenderStack
   - RenderIndexedStack

### Phase 4: Advanced Features (неделя 5)
1. Layout Strategy traits
2. Hit Test Strategy traits
3. Paint Strategy traits
4. Оптимизации и документация

### Phase 5: Polish & Documentation (неделя 6)
1. Comprehensive documentation
2. Migration guide
3. Examples
4. Performance benchmarks

---

## Преимущества решения

### 1. DRY (Don't Repeat Yourself)
✅ Один `SingleChildRenderCore` вместо 50 копий
✅ Derive macros генерируют код автоматически
✅ Изменение в одном месте → изменение везде

### 2. Zero-Cost Abstractions
✅ Все #[inline] методы → compiler inlining
✅ Generic specialization → monomorphization
✅ Нет runtime overhead от composition

### 3. Type Safety
✅ Compiler проверяет всё на этапе компиляции
✅ Невозможно забыть реализовать метод
✅ Ясные интерфейсы через traits

### 4. Maintainability
✅ Меньше кода = меньше bugs
✅ Консистентность между всеми RenderObject'ами
✅ Легко добавлять новые типы

### 5. Performance
✅ Сохраняется кэширование layout
✅ RenderFlags экономит память
✅ Inline оптимизации

### 6. Extensibility
✅ Strategy traits для кастомизации
✅ Macros можно расширять
✅ Core можно эволюционировать

---

## Альтернативы и их недостатки

### ❌ Alt 1: Trait-based hierarchy
```rust
trait RenderObjectBase {
    fn element_id(&self) -> Option<ElementId>;
    // ...
}
```
**Проблема:** Нет default implementations для полей, нужно impl в каждом типе

### ❌ Alt 2: Macro-only solution
```rust
render_object! {
    struct RenderOpacity { ... }
}
```
**Проблема:** Теряется читаемость, трудно debug, IDE support плохой

### ❌ Alt 3: Base struct inheritance
```rust
struct RenderBase { ... }
struct RenderOpacity { base: RenderBase, ... }
```
**Проблема:** Неявное делегирование, нужны getter методы для всего

### ✅ Наше решение: Composition + Derive Macros
- Читаемый код
- IDE support
- Явное делегирование через derive
- Zero-cost abstractions

---

## Заключение

Это решение:
1. **Устраняет 85% дублирования кода** (~15,000 lines)
2. **Сохраняет zero-cost** производительность
3. **Улучшает maintainability** через DRY
4. **Легко мигрируется** постепенно
5. **Расширяемое** для будущих нужд

Это **правильная** Rust архитектура, использующая:
- Composition over inheritance
- Zero-cost abstractions
- Procedural macros для кодогенерации
- Type system для гарантий
