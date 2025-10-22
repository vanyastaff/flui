// ============================================================================
// ПРОДВИНУТЫЕ ПАТТЕРНЫ: Стратегии и MultiChildRenderCore
// ============================================================================

use std::marker::PhantomData;

// ============================================================================
// PART 1: Layout Strategy Pattern
// ============================================================================

/// Trait для различных стратегий layout
/// 
/// Позволяет переиспользовать логику layout между разными RenderObject'ами
pub trait LayoutStrategy: Send + Sync {
    /// Вычислить layout и вернуть размер
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        constraints: BoxConstraints,
    ) -> Size;
}

/// Passthrough layout - просто передать constraints ребенку
#[derive(Debug, Clone, Copy, Default)]
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

/// Modified constraints layout - модифицировать constraints перед передачей
#[derive(Debug)]
pub struct ModifiedConstraintsLayout<F>
where
    F: Fn(BoxConstraints) -> BoxConstraints + Send + Sync,
{
    modifier: F,
}

impl<F> ModifiedConstraintsLayout<F>
where
    F: Fn(BoxConstraints) -> BoxConstraints + Send + Sync,
{
    pub fn new(modifier: F) -> Self {
        Self { modifier }
    }
}

impl<F> LayoutStrategy for ModifiedConstraintsLayout<F>
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
            constraints.constrain(child_size)
        } else {
            constraints.smallest()
        }
    }
}

/// Tight constraints layout - всегда tight constraints
#[derive(Debug, Clone, Copy)]
pub struct TightLayout {
    pub width: f32,
    pub height: f32,
}

impl LayoutStrategy for TightLayout {
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        _constraints: BoxConstraints,
    ) -> Size {
        let tight = BoxConstraints::tight(Size::new(self.width, self.height));
        if let Some(child) = core.child_mut() {
            child.layout(tight);
        }
        Size::new(self.width, self.height)
    }
}

/// Aspect ratio layout - сохранять aspect ratio
#[derive(Debug, Clone, Copy)]
pub struct AspectRatioLayout {
    pub aspect_ratio: f32,
}

impl LayoutStrategy for AspectRatioLayout {
    fn compute_layout(
        &self,
        core: &mut SingleChildRenderCore,
        constraints: BoxConstraints,
    ) -> Size {
        // Calculate size that fits constraints and maintains aspect ratio
        let max_width = constraints.max_width;
        let max_height = constraints.max_height;
        
        let width = max_width.min(max_height * self.aspect_ratio);
        let height = width / self.aspect_ratio;
        
        let size = Size::new(width, height);
        let tight = BoxConstraints::tight(size);
        
        if let Some(child) = core.child_mut() {
            child.layout(tight);
        }
        
        size
    }
}

// ============================================================================
// PART 2: Hit Test Strategy Pattern
// ============================================================================

/// Trait для различных стратегий hit testing
pub trait HitTestStrategy: Send + Sync {
    /// Выполнить hit test
    fn hit_test(
        &self,
        core: &SingleChildRenderCore,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool;
}

/// Standard hit test - bounds check + children
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardHitTest;

impl HitTestStrategy for StandardHitTest {
    fn hit_test(
        &self,
        core: &SingleChildRenderCore,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        core.hit_test_default(result, position, true)
    }
}

/// Transparent hit test - никогда не реагировать
#[derive(Debug, Clone, Copy, Default)]
pub struct TransparentHitTest;

impl HitTestStrategy for TransparentHitTest {
    fn hit_test(
        &self,
        core: &SingleChildRenderCore,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        // Pass through to children only
        core.hit_test_child(result, position)
    }
}

/// Absorb hit test - блокировать все события
#[derive(Debug, Clone, Copy, Default)]
pub struct AbsorbHitTest;

impl HitTestStrategy for AbsorbHitTest {
    fn hit_test(
        &self,
        core: &SingleChildRenderCore,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        // Bounds check
        let size = core.size();
        if position.dx < 0.0
            || position.dx >= size.width
            || position.dy < 0.0
            || position.dy >= size.height
        {
            return false;
        }
        
        // Absorb - don't check children, just add self
        result.add(HitTestEntry::new(position, size));
        true
    }
}

/// Conditional hit test - проверять условие
#[derive(Debug)]
pub struct ConditionalHitTest<F>
where
    F: Fn() -> bool + Send + Sync,
{
    condition: F,
}

impl<F> ConditionalHitTest<F>
where
    F: Fn() -> bool + Send + Sync,
{
    pub fn new(condition: F) -> Self {
        Self { condition }
    }
}

impl<F> HitTestStrategy for ConditionalHitTest<F>
where
    F: Fn() -> bool + Send + Sync,
{
    fn hit_test(
        &self,
        core: &SingleChildRenderCore,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if (self.condition)() {
            core.hit_test_default(result, position, true)
        } else {
            false
        }
    }
}

// ============================================================================
// PART 3: Paint Strategy Pattern
// ============================================================================

/// Trait для различных стратегий paint
pub trait PaintStrategy: Send + Sync {
    /// Нарисовать
    fn paint(
        &self,
        core: &SingleChildRenderCore,
        painter: &egui::Painter,
        offset: Offset,
    );
}

/// Standard paint - просто нарисовать child
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardPaint;

impl PaintStrategy for StandardPaint {
    fn paint(
        &self,
        core: &SingleChildRenderCore,
        painter: &egui::Painter,
        offset: Offset,
    ) {
        core.paint_child(painter, offset);
    }
}

/// Offset paint - нарисовать с offset
#[derive(Debug, Clone, Copy)]
pub struct OffsetPaint {
    pub dx: f32,
    pub dy: f32,
}

impl PaintStrategy for OffsetPaint {
    fn paint(
        &self,
        core: &SingleChildRenderCore,
        painter: &egui::Painter,
        offset: Offset,
    ) {
        let modified_offset = offset + Offset::new(self.dx, self.dy);
        core.paint_child(painter, modified_offset);
    }
}

/// Clipped paint - нарисовать с clipping
#[derive(Debug, Clone, Copy)]
pub struct ClippedPaint {
    pub clip_behavior: Clip,
}

impl PaintStrategy for ClippedPaint {
    fn paint(
        &self,
        core: &SingleChildRenderCore,
        painter: &egui::Painter,
        offset: Offset,
    ) {
        if self.clip_behavior != Clip::None {
            let size = core.size();
            let rect = Rect::from_min_size(
                egui::pos2(offset.dx, offset.dy),
                egui::vec2(size.width, size.height),
            );
            painter.set_clip_rect(rect);
        }
        core.paint_child(painter, offset);
    }
}

// ============================================================================
// PART 4: Strategy-based RenderObject
// ============================================================================

/// Generic RenderObject с настраиваемыми стратегиями
/// 
/// Позволяет создавать RenderObject'ы без написания нового кода
pub struct StrategyRenderObject<L, H, P>
where
    L: LayoutStrategy,
    H: HitTestStrategy,
    P: PaintStrategy,
{
    core: SingleChildRenderCore,
    layout_strategy: L,
    hit_test_strategy: H,
    paint_strategy: P,
}

impl<L, H, P> StrategyRenderObject<L, H, P>
where
    L: LayoutStrategy,
    H: HitTestStrategy,
    P: PaintStrategy,
{
    pub fn new(
        layout_strategy: L,
        hit_test_strategy: H,
        paint_strategy: P,
    ) -> Self {
        Self {
            core: SingleChildRenderCore::new(),
            layout_strategy,
            hit_test_strategy,
            paint_strategy,
        }
    }
    
    pub fn with_element_id(
        element_id: ElementId,
        layout_strategy: L,
        hit_test_strategy: H,
        paint_strategy: P,
    ) -> Self {
        Self {
            core: SingleChildRenderCore::with_element_id(element_id),
            layout_strategy,
            hit_test_strategy,
            paint_strategy,
        }
    }
}

impl<L, H, P> DynRenderObject for StrategyRenderObject<L, H, P>
where
    L: LayoutStrategy,
    H: HitTestStrategy,
    P: PaintStrategy,
{
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        impl_cached_layout!(self.core, constraints, {
            self.layout_strategy.compute_layout(&mut self.core, constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        self.paint_strategy.paint(&self.core, painter, offset);
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        self.hit_test_strategy.hit_test(&self.core, result, position)
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
// PART 5: MultiChildRenderCore
// ============================================================================

/// Core для multi-child render objects (Flex, Stack, etc.)
#[derive(Debug)]
pub struct MultiChildRenderCore<P: ParentData> {
    /// Element ID для кэш-инвалидации
    pub element_id: Option<ElementId>,
    
    /// Children с parent data
    pub children: Vec<ChildEntry<P>>,
    
    /// Размер после layout
    pub size: Size,
    
    /// Текущие constraints
    pub constraints: Option<BoxConstraints>,
    
    /// Битфлаги состояния
    pub flags: RenderFlags,
}

/// Entry для child в multi-child layout
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
    
    pub const fn with_element_id(element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            children: Vec::new(),
            size: Size::ZERO,
            constraints: None,
            flags: RenderFlags::new(),
        }
    }
    
    // ========================================================================
    // Child Management
    // ========================================================================
    
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
    
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
    
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>, parent_data: P) {
        self.children.push(ChildEntry {
            render_object: child,
            parent_data,
            offset: Offset::ZERO,
        });
        self.flags.mark_needs_layout();
    }
    
    pub fn insert_child(&mut self, index: usize, child: Box<dyn DynRenderObject>, parent_data: P) {
        self.children.insert(
            index,
            ChildEntry {
                render_object: child,
                parent_data,
                offset: Offset::ZERO,
            },
        );
        self.flags.mark_needs_layout();
    }
    
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn DynRenderObject>> {
        if index < self.children.len() {
            let entry = self.children.remove(index);
            self.flags.mark_needs_layout();
            Some(entry.render_object)
        } else {
            None
        }
    }
    
    pub fn clear_children(&mut self) {
        if !self.children.is_empty() {
            self.children.clear();
            self.flags.mark_needs_layout();
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
    
    // ========================================================================
    // Visitor Patterns
    // ========================================================================
    
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
    
    // ========================================================================
    // Paint Helpers
    // ========================================================================
    
    pub fn paint_children(&self, painter: &egui::Painter, offset: Offset) {
        for child in &self.children {
            let child_offset = offset + child.offset;
            child.render_object.paint(painter, child_offset);
        }
    }
    
    // ========================================================================
    // Hit Test Helpers
    // ========================================================================
    
    pub fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        // Hit test in reverse order (front to back)
        for child in self.children.iter().rev() {
            let local_position = position - child.offset;
            if child.render_object.hit_test(result, local_position) {
                return true;
            }
        }
        false
    }
}

impl<P: ParentData> Default for MultiChildRenderCore<P> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PART 6: Example - RenderFlex using MultiChildRenderCore
// ============================================================================

#[derive(Debug)]
pub struct RenderFlex {
    core: MultiChildRenderCore<FlexParentData>,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            core: MultiChildRenderCore::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }
    
    pub fn with_element_id(element_id: ElementId, direction: Axis) -> Self {
        Self {
            core: MultiChildRenderCore::with_element_id(element_id),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }
    
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>, parent_data: FlexParentData) {
        self.core.add_child(child, parent_data);
    }
    
    pub fn set_main_axis_alignment(&mut self, alignment: MainAxisAlignment) {
        if self.main_axis_alignment != alignment {
            self.main_axis_alignment = alignment;
            self.core.mark_needs_layout();
        }
    }
    
    pub fn set_cross_axis_alignment(&mut self, alignment: CrossAxisAlignment) {
        if self.cross_axis_alignment != alignment {
            self.cross_axis_alignment = alignment;
            self.core.mark_needs_layout();
        }
    }
    
    fn perform_flex_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Сложная логика flex layout
        // (упрощенная версия для примера)
        
        let mut main_size = 0.0;
        let mut cross_size = 0.0;
        
        for child in &mut self.core.children {
            // Layout каждого child'а
            let child_constraints = match self.direction {
                Axis::Horizontal => BoxConstraints::new(
                    0.0,
                    constraints.max_width,
                    constraints.min_height,
                    constraints.max_height,
                ),
                Axis::Vertical => BoxConstraints::new(
                    constraints.min_width,
                    constraints.max_width,
                    0.0,
                    constraints.max_height,
                ),
            };
            
            let child_size = child.render_object.layout(child_constraints);
            
            match self.direction {
                Axis::Horizontal => {
                    child.offset = Offset::new(main_size, 0.0);
                    main_size += child_size.width;
                    cross_size = cross_size.max(child_size.height);
                }
                Axis::Vertical => {
                    child.offset = Offset::new(0.0, main_size);
                    main_size += child_size.height;
                    cross_size = cross_size.max(child_size.width);
                }
            }
        }
        
        let size = match self.direction {
            Axis::Horizontal => Size::new(main_size, cross_size),
            Axis::Vertical => Size::new(cross_size, main_size),
        };
        
        constraints.constrain(size)
    }
}

impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Используем макрос с child_count для multi-child
        impl_cached_layout!(self.core, constraints, self.core.child_count(), {
            self.perform_flex_layout(constraints)
        })
    }
    
    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        self.core.paint_children(painter, offset);
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Bounds check
        if position.dx < 0.0
            || position.dx >= self.core.size().width
            || position.dy < 0.0
            || position.dy >= self.core.size().height
        {
            return false;
        }
        
        self.core.hit_test_children(result, position)
    }
    
    // Standard delegation
    fn size(&self) -> Size { self.core.size() }
    fn constraints(&self) -> Option<BoxConstraints> { self.core.constraints() }
    fn needs_layout(&self) -> bool { self.core.needs_layout() }
    fn mark_needs_layout(&mut self) { self.core.mark_needs_layout() }
    fn needs_paint(&self) -> bool { self.core.needs_paint() }
    fn mark_needs_paint(&mut self) { self.core.mark_needs_paint() }
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        self.core.visit_children(visitor)
    }
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        self.core.visit_children_mut(visitor)
    }
}

// ============================================================================
// PART 7: Usage Examples
// ============================================================================

#[cfg(test)]
mod examples {
    use super::*;
    
    /// Example 1: Create RenderOpacity using strategies
    fn example_render_opacity() {
        let render = StrategyRenderObject::new(
            PassthroughLayout,
            ConditionalHitTest::new(|| true), // not transparent
            StandardPaint,
        );
    }
    
    /// Example 2: Create RenderClipRect using strategies
    fn example_render_clip_rect() {
        let render = StrategyRenderObject::new(
            PassthroughLayout,
            StandardHitTest,
            ClippedPaint { clip_behavior: Clip::AntiAlias },
        );
    }
    
    /// Example 3: Create RenderPadding using strategies
    fn example_render_padding() {
        let padding = EdgeInsets::all(10.0);
        let render = StrategyRenderObject::new(
            ModifiedConstraintsLayout::new(move |c| c.deflate(padding)),
            StandardHitTest,
            OffsetPaint { dx: padding.left, dy: padding.top },
        );
    }
    
    /// Example 4: Create RenderFlex
    fn example_render_flex() {
        let mut flex = RenderFlex::new(Axis::Horizontal);
        
        flex.add_child(
            Box::new(RenderBox::new()),
            FlexParentData::new(),
        );
        
        flex.add_child(
            Box::new(RenderBox::new()),
            FlexParentData::with_flex(1),
        );
    }
}

// ============================================================================
// SUMMARY: Что мы получили
// ============================================================================

/*

1. **SingleChildRenderCore** - универсальное ядро для single-child
   - Все общие поля в одном месте
   - Переиспользуемые методы (layout_passthrough, paint_child, etc.)
   - Zero-cost abstractions (#[inline])

2. **MultiChildRenderCore** - универсальное ядро для multi-child
   - Управление списком children
   - Parent data поддержка
   - Visitor patterns

3. **Strategy Pattern** - композируемое поведение
   - LayoutStrategy - различные алгоритмы layout
   - HitTestStrategy - различные способы hit testing
   - PaintStrategy - различные способы рисования
   
4. **StrategyRenderObject** - generic RenderObject
   - Создание RenderObject'ов без нового кода
   - Полная кастомизация через стратегии
   - Type-safe composition

5. **Экономия кода:**
   - SingleChild: ~150 lines → ~70 lines (50% reduction)
   - MultiChild: ~200 lines → ~100 lines (50% reduction)
   - Strategy-based: ~0 lines (полная генерация)

6. **Производительность:**
   - Zero-cost: все #[inline], compiler оптимизирует
   - Кэширование сохранено: impl_cached_layout! работает
   - Memory: та же память (composition без overhead)

7. **Maintainability:**
   - DRY: изменения в одном месте
   - Консистентность: все используют одинаковые паттерны
   - Extensibility: легко добавлять новые стратегии

*/
