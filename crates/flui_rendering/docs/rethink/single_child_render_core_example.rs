// ============================================================================
// ПРИМЕР РЕАЛИЗАЦИИ: SingleChildRenderCore и производные
// ============================================================================

/// Базовое ядро для всех single-child RenderObject'ов
/// 
/// Содержит все общие поля и методы, которые повторяются в каждом RenderObject.
/// Использование через composition устраняет дублирование кода.
#[derive(Debug, Clone)]
pub struct SingleChildRenderCore {
    /// Element ID для кэш-инвалидации
    pub element_id: Option<ElementId>,
    
    /// Child render object
    pub child: Option<Box<dyn DynRenderObject>>,
    
    /// Размер после layout
    pub size: Size,
    
    /// Текущие constraints
    pub constraints: Option<BoxConstraints>,
    
    /// Битфлаги состояния (1 byte!)
    pub flags: RenderFlags,
}

impl SingleChildRenderCore {
    /// Создать новое ядро с default значениями
    #[inline]
    pub const fn new() -> Self {
        Self {
            element_id: None,
            child: None,
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    /// Создать с ElementId для кэширования
    #[inline]
    pub const fn with_element_id(element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            child: None,
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    // ========================================================================
    // Element ID Management
    // ========================================================================
    
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }
    
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
    
    // ========================================================================
    // Child Management
    // ========================================================================
    
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
    
    // ========================================================================
    // Layout State
    // ========================================================================
    
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
    
    // ========================================================================
    // Flags Management
    // ========================================================================
    
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
    
    #[inline]
    pub fn flags(&self) -> RenderFlags {
        self.flags
    }
    
    #[inline]
    pub fn flags_mut(&mut self) -> &mut RenderFlags {
        &mut self.flags
    }
    
    // ========================================================================
    // Common Layout Patterns
    // ========================================================================
    
    /// Passthrough layout - просто передать constraints ребенку
    /// 
    /// Используется в RenderOpacity, RenderClipRect, и других simple wrappers
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
    
    /// Layout с модификацией child constraints
    /// 
    /// Используется в RenderPadding (deflate), RenderConstrainedBox, etc.
    pub fn layout_with_modified_constraints<F>(
        &mut self,
        constraints: BoxConstraints,
        modify: F,
    ) -> Size
    where
        F: FnOnce(BoxConstraints) -> BoxConstraints,
    {
        let modified_constraints = modify(constraints);
        
        if let Some(child) = &mut self.child {
            self.size = child.layout(modified_constraints);
        } else {
            self.size = modified_constraints.smallest();
        }
        
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    /// Layout с post-processing размера
    /// 
    /// Используется когда нужно изменить размер после layout child'а
    pub fn layout_with_post_process<F>(
        &mut self,
        constraints: BoxConstraints,
        post_process: F,
    ) -> Size
    where
        F: FnOnce(Size, &dyn DynRenderObject) -> Size,
    {
        if let Some(child) = &mut self.child {
            let child_size = child.layout(constraints);
            self.size = post_process(child_size, child.as_ref());
        } else {
            self.size = constraints.smallest();
        }
        
        self.constraints = Some(constraints);
        self.flags.clear_needs_layout();
        self.size
    }
    
    // ========================================================================
    // Common Visitor Patterns
    // ========================================================================
    
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
    
    // ========================================================================
    // Common Hit Test Patterns
    // ========================================================================
    
    /// Делегировать hit test к child'у
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
    
    /// Default hit test с bounds checking
    pub fn hit_test_default(
        &self,
        result: &mut HitTestResult,
        position: Offset,
        hit_self: bool,
    ) -> bool {
        // Bounds check
        if position.dx < 0.0
            || position.dx >= self.size.width
            || position.dy < 0.0
            || position.dy >= self.size.height
        {
            return false;
        }
        
        // Check children first (front-to-back)
        let hit_child = self.hit_test_child(result, position);
        
        // Add to result if we hit child or self
        if hit_child || hit_self {
            result.add(HitTestEntry::new(position, self.size));
            return true;
        }
        
        false
    }
    
    // ========================================================================
    // Common Paint Patterns
    // ========================================================================
    
    /// Просто нарисовать child с offset
    #[inline]
    pub fn paint_child(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }
    
    /// Paint child с модифицированным offset
    #[inline]
    pub fn paint_child_with_offset<F>(
        &self,
        painter: &egui::Painter,
        offset: Offset,
        modify_offset: F,
    ) where
        F: FnOnce(Offset) -> Offset,
    {
        if let Some(child) = &self.child {
            let modified_offset = modify_offset(offset);
            child.paint(painter, modified_offset);
        }
    }
}

impl Default for SingleChildRenderCore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ПРИМЕР 1: RenderOpacity с использованием SingleChildRenderCore
// ============================================================================

/// RenderOpacity - применяет прозрачность к child'у
/// 
/// **ДО рефакторинга:** 150+ lines кода
/// **ПОСЛЕ рефакторинга:** 70 lines кода
/// **Экономия:** 53% меньше кода
#[derive(Debug)]
pub struct RenderOpacity {
    /// Core содержит все общие поля и методы
    core: SingleChildRenderCore,
    
    /// Специфичное поле: прозрачность (0.0-1.0)
    opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        assert!((0.0..=1.0).contains(&opacity), "opacity must be in [0.0, 1.0]");
        Self {
            core: SingleChildRenderCore::new(),
            opacity,
        }
    }
    
    pub fn with_element_id(element_id: ElementId, opacity: f32) -> Self {
        assert!((0.0..=1.0).contains(&opacity), "opacity must be in [0.0, 1.0]");
        Self {
            core: SingleChildRenderCore::with_element_id(element_id),
            opacity,
        }
    }
    
    // Делегирование к core (в будущем через derive macro)
    #[inline] pub fn element_id(&self) -> Option<ElementId> { self.core.element_id() }
    #[inline] pub fn set_element_id(&mut self, id: Option<ElementId>) { self.core.set_element_id(id) }
    #[inline] pub fn child(&self) -> Option<&dyn DynRenderObject> { self.core.child() }
    #[inline] pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) { self.core.set_child(child) }
    #[inline] pub fn mark_needs_layout(&mut self) { self.core.mark_needs_layout() }
    #[inline] pub fn mark_needs_paint(&mut self) { self.core.mark_needs_paint() }
    #[inline] pub fn needs_layout(&self) -> bool { self.core.needs_layout() }
    #[inline] pub fn needs_paint(&self) -> bool { self.core.needs_paint() }
    
    // Специфичные методы
    pub fn set_opacity(&mut self, opacity: f32) {
        assert!((0.0..=1.0).contains(&opacity), "opacity must be in [0.0, 1.0]");
        if (self.opacity - opacity).abs() > f32::EPSILON {
            self.opacity = opacity;
            self.core.mark_needs_paint(); // Только repaint, не relayout
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

impl DynRenderObject for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Используем существующий макрос с core
        impl_cached_layout!(self.core, constraints, {
            self.core.layout_passthrough(constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Специфичная логика: skip если transparent
        if !self.is_transparent() {
            self.core.paint_child(painter, offset);
        }
    }
    
    fn hit_test_self(&self, _position: Offset) -> bool {
        // Transparent objects не реагируют на hit tests
        !self.is_transparent()
    }
    
    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if self.is_transparent() {
            return false;
        }
        self.core.hit_test_child(result, position)
    }
    
    // Все остальные методы делегируются к core
    fn size(&self) -> Size { self.core.size() }
    fn constraints(&self) -> Option<BoxConstraints> { self.core.constraints() }
    fn needs_layout(&self) -> bool { self.core.needs_layout() }
    fn mark_needs_layout(&mut self) { self.core.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.core.needs_paint() }
    fn mark_needs_paint(&mut self) { self.core.mark_needs_paint() }
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        self.core.visit_child(visitor)
    }
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        self.core.visit_child_mut(visitor)
    }
}

// ============================================================================
// ПРИМЕР 2: RenderPadding с modified constraints
// ============================================================================

#[derive(Debug)]
pub struct RenderPadding {
    core: SingleChildRenderCore,
    padding: EdgeInsets,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            core: SingleChildRenderCore::new(),
            padding,
        }
    }
    
    pub fn with_element_id(element_id: ElementId, padding: EdgeInsets) -> Self {
        Self {
            core: SingleChildRenderCore::with_element_id(element_id),
            padding,
        }
    }
    
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        if self.padding != padding {
            self.padding = padding;
            self.core.mark_needs_layout();
        }
    }
    
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
    
    // Делегирование методов (в будущем через derive macro)
    #[inline] pub fn element_id(&self) -> Option<ElementId> { self.core.element_id() }
    #[inline] pub fn set_element_id(&mut self, id: Option<ElementId>) { self.core.set_element_id(id) }
    #[inline] pub fn child(&self) -> Option<&dyn DynRenderObject> { self.core.child() }
    #[inline] pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) { self.core.set_child(child) }
}

impl DynRenderObject for RenderPadding {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self.core, constraints, {
            // Используем helper для modified constraints
            let padding = self.padding;
            self.core.layout_with_post_process(
                constraints.deflate(padding),
                |child_size, _| {
                    // Add padding back to get final size
                    Size::new(
                        child_size.width + padding.horizontal(),
                        child_size.height + padding.vertical(),
                    )
                },
            )
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child с offset учитывающим padding
        self.core.paint_child_with_offset(painter, offset, |o| {
            o + Offset::new(self.padding.left, self.padding.top)
        })
    }
    
    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        // Adjust position for padding
        let child_position = position - Offset::new(self.padding.left, self.padding.top);
        self.core.hit_test_child(result, child_position)
    }
    
    // Standard delegation
    fn size(&self) -> Size { self.core.size() }
    fn constraints(&self) -> Option<BoxConstraints> { self.core.constraints() }
    fn needs_layout(&self) -> bool { self.core.needs_layout() }
    fn mark_needs_layout(&mut self) { self.core.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.core.needs_paint() }
    fn mark_needs_paint(&mut self) { self.core.mark_needs_paint() }
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        self.core.visit_child(visitor)
    }
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        self.core.visit_child_mut(visitor)
    }
}

// ============================================================================
// МАКРОС ДЛЯ ДЕЛЕГИРОВАНИЯ (упрощенная версия)
// ============================================================================

/// Макрос для генерации делегирующих методов
/// 
/// В production это будет derive macro, но для примера используем declarative
macro_rules! delegate_to_core {
    ($core:ident) => {
        #[inline]
        pub fn element_id(&self) -> Option<ElementId> {
            self.$core.element_id()
        }
        
        #[inline]
        pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
            self.$core.set_element_id(element_id)
        }
        
        #[inline]
        pub fn child(&self) -> Option<&dyn DynRenderObject> {
            self.$core.child()
        }
        
        #[inline]
        pub fn child_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
            self.$core.child_mut()
        }
        
        #[inline]
        pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
            self.$core.set_child(child)
        }
        
        #[inline]
        pub fn take_child(&mut self) -> Option<Box<dyn DynRenderObject>> {
            self.$core.take_child()
        }
        
        #[inline]
        pub fn size(&self) -> Size {
            self.$core.size()
        }
        
        #[inline]
        pub fn constraints(&self) -> Option<BoxConstraints> {
            self.$core.constraints()
        }
        
        #[inline]
        pub fn needs_layout(&self) -> bool {
            self.$core.needs_layout()
        }
        
        #[inline]
        pub fn needs_paint(&self) -> bool {
            self.$core.needs_paint()
        }
        
        #[inline]
        pub fn mark_needs_layout(&mut self) {
            self.$core.mark_needs_layout()
        }
        
        #[inline]
        pub fn mark_needs_paint(&mut self) {
            self.$core.mark_needs_paint()
        }
    };
}

/// Макрос для автоматической имплементации DynRenderObject базовых методов
macro_rules! impl_dyn_render_object_base {
    ($ty:ty, $core:ident) => {
        impl DynRenderObject for $ty {
            fn size(&self) -> Size {
                self.$core.size()
            }
            
            fn constraints(&self) -> Option<BoxConstraints> {
                self.$core.constraints()
            }
            
            fn needs_layout(&self) -> bool {
                self.$core.needs_layout()
            }
            
            fn mark_needs_layout(&mut self) {
                self.$core.mark_needs_layout()
            }
            
            fn needs_paint(&self) -> bool {
                self.$core.needs_paint()
            }
            
            fn mark_needs_paint(&mut self) {
                self.$core.mark_needs_paint()
            }
            
            fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
                self.$core.visit_child(visitor)
            }
            
            fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
                self.$core.visit_child_mut(visitor)
            }
            
            fn hit_test_children(
                &self,
                result: &mut HitTestResult,
                position: Offset,
            ) -> bool {
                self.$core.hit_test_child(result, position)
            }
            
            // layout() и paint() должны быть реализованы вручную
            // так как они специфичны для каждого типа
        }
    };
}

// ============================================================================
// ПРИМЕР 3: RenderClipRect с использованием макроса
// ============================================================================

#[derive(Debug)]
pub struct RenderClipRect {
    core: SingleChildRenderCore,
    clip_behavior: Clip,
}

impl RenderClipRect {
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            core: SingleChildRenderCore::new(),
            clip_behavior,
        }
    }
    
    // Генерируем все делегирующие методы одной строкой:
    delegate_to_core!(core);
    
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        if self.clip_behavior != clip_behavior {
            self.clip_behavior = clip_behavior;
            self.core.mark_needs_paint();
        }
    }
    
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }
}

impl DynRenderObject for RenderClipRect {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self.core, constraints, {
            self.core.layout_passthrough(constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if self.clip_behavior != Clip::None {
            // Apply clipping (egui specific)
            let rect = Rect::from_min_size(
                egui::pos2(offset.dx, offset.dy),
                egui::vec2(self.core.size().width, self.core.size().height),
            );
            painter.set_clip_rect(rect);
        }
        self.core.paint_child(painter, offset);
    }
    
    // Остальные методы генерируются автоматически через наследование
    // от базовой имплементации (будущая feature в macro)
    fn size(&self) -> Size { self.core.size() }
    fn constraints(&self) -> Option<BoxConstraints> { self.core.constraints() }
    fn needs_layout(&self) -> bool { self.core.needs_layout() }
    fn mark_needs_layout(&mut self) { self.core.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.core.needs_paint() }
    fn mark_needs_paint(&mut self) { self.core.mark_needs_paint() }
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        self.core.visit_child(visitor)
    }
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        self.core.visit_child_mut(visitor)
    }
    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool {
        self.core.hit_test_child(result, position)
    }
}

// ============================================================================
// ТЕСТЫ
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_single_child_core_new() {
        let core = SingleChildRenderCore::new();
        assert!(core.element_id.is_none());
        assert!(core.child.is_none());
        assert!(core.needs_layout());
        assert!(core.needs_paint());
    }
    
    #[test]
    fn test_single_child_core_with_element_id() {
        let id = ElementId::new();
        let core = SingleChildRenderCore::with_element_id(id);
        assert_eq!(core.element_id, Some(id));
    }
    
    #[test]
    fn test_render_opacity_creation() {
        let opacity = RenderOpacity::new(0.5);
        assert_eq!(opacity.opacity(), 0.5);
        assert!(!opacity.is_transparent());
        assert!(!opacity.is_opaque());
    }
    
    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = RenderOpacity::new(0.5);
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity(), 0.8);
        assert!(opacity.needs_paint());
    }
    
    #[test]
    fn test_render_padding_layout() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        let child = Box::new(RenderBox::new());
        padding.set_child(Some(child));
        
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = padding.layout(constraints);
        
        // Child gets 80x80 (100 - 20 for padding), final size is 100x100
        assert_eq!(size, Size::new(100.0, 100.0));
    }
    
    #[test]
    fn test_delegate_macro() {
        let mut clip = RenderClipRect::new(Clip::AntiAlias);
        
        // Test delegated methods работают
        assert!(clip.element_id().is_none());
        assert!(clip.child().is_none());
        
        let id = ElementId::new();
        clip.set_element_id(Some(id));
        assert_eq!(clip.element_id(), Some(id));
    }
}
