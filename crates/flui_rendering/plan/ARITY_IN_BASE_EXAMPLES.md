# Вариант 3: Arity в базовой структуре

Arity определяется в базовой структуре, trait `RenderBox` без generic параметра.

## Базовые структуры

```rust
use flui_tree::{Arity, Children, Leaf, Single, Optional, Variable};

// ============================================================================
// БАЗОВЫЕ СТРУКТУРЫ С ARITY
// ============================================================================

/// База для любого Box (хранит size + children)
pub struct BoxBase<A: Arity> {
    pub children: Children<RenderNodeId, A>,
    pub size: Size,
}

impl<A: Arity> BoxBase<A> {
    pub fn new() -> Self {
        Self {
            children: Children::new(),
            size: Size::ZERO,
        }
    }
}

/// База для shifted box (single child + offset)
pub struct ShiftedBoxBase {
    pub inner: BoxBase<Single>,
    pub child_offset: Offset,
}

impl ShiftedBoxBase {
    pub fn new() -> Self {
        Self {
            inner: BoxBase::new(),
            child_offset: Offset::ZERO,
        }
    }
    
    pub fn child(&self) -> Option<RenderNodeId> {
        self.inner.children.get()
    }
    
    pub fn paint_child(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            ctx.paint_child(child, offset + self.child_offset);
        }
    }
}

/// База для optional shifted box (0-1 child + offset)
pub struct OptionalShiftedBoxBase {
    pub inner: BoxBase<Optional>,
    pub child_offset: Offset,
}

impl OptionalShiftedBoxBase {
    pub fn new() -> Self {
        Self {
            inner: BoxBase::new(),
            child_offset: Offset::ZERO,
        }
    }
    
    pub fn child(&self) -> Option<RenderNodeId> {
        self.inner.children.get()
    }
    
    pub fn paint_child(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            ctx.paint_child(child, offset + self.child_offset);
        }
    }
}

/// База для aligning box (optional child + alignment)
pub struct AligningBoxBase {
    pub inner: OptionalShiftedBoxBase,
    pub alignment: Alignment,
}

impl AligningBoxBase {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            inner: OptionalShiftedBoxBase::new(),
            alignment,
        }
    }
    
    pub fn child(&self) -> Option<RenderNodeId> {
        self.inner.child()
    }
    
    pub fn align_child(&mut self, child_size: Size, container_size: Size) {
        self.inner.child_offset = self.alignment.compute_offset(child_size, container_size);
    }
    
    pub fn paint_child(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.inner.paint_child(ctx, offset);
    }
}

/// База для proxy box (single child, делегирует всё)
pub struct ProxyBoxBase {
    pub inner: BoxBase<Single>,
}

impl ProxyBoxBase {
    pub fn new() -> Self {
        Self { inner: BoxBase::new() }
    }
    
    pub fn child(&self) -> Option<RenderNodeId> {
        self.inner.children.get()
    }
    
    pub fn paint_child(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            ctx.paint_child(child, offset);
        }
    }
}

/// База для контейнера (variable children)
pub struct ContainerBoxBase {
    pub inner: BoxBase<Variable>,
}

impl ContainerBoxBase {
    pub fn new() -> Self {
        Self { inner: BoxBase::new() }
    }
    
    pub fn children(&self) -> impl Iterator<Item = RenderNodeId> + '_ {
        self.inner.children.iter()
    }
    
    pub fn child_count(&self) -> usize {
        self.inner.children.len()
    }
}
```

## Главный Trait (без Arity!)

```rust
// ============================================================================
// ОДИН TRAIT БЕЗ GENERIC ПАРАМЕТРА
// ============================================================================

pub trait RenderBox: RenderObject {
    /// Arity определяется через associated type
    type Arity: Arity;
    
    /// Layout - обязательный
    fn perform_layout(
        &mut self,
        constraints: &BoxConstraints,
        layout: &mut LayoutHelper,
    ) -> Size;
    
    /// Paint - обязательный  
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    
    /// Hit test - с дефолтом
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // default implementation
        true
    }
    
    /// Intrinsics - опциональные
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
}
```

---

## Пример 1: RenderAlign (Optional child)

```rust
// ============================================================================
// RENDER ALIGN - Optional child + alignment
// ============================================================================

pub struct RenderAlign {
    base: AligningBoxBase,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
}

impl RenderAlign {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: AligningBoxBase::new(alignment),
            width_factor: None,
            height_factor: None,
        }
    }
    
    pub fn centered() -> Self {
        Self::new(Alignment::CENTER)
    }
    
    pub fn with_factors(alignment: Alignment, wf: Option<f32>, hf: Option<f32>) -> Self {
        Self {
            base: AligningBoxBase::new(alignment),
            width_factor: wf,
            height_factor: hf,
        }
    }
}

impl RenderBox for RenderAlign {
    type Arity = Optional;
    
    fn perform_layout(&mut self, constraints: &BoxConstraints, layout: &mut LayoutHelper) -> Size {
        if let Some(child) = self.base.child() {
            let child_size = layout.layout_child(child, constraints.loosen());
            
            let size = Size::new(
                self.width_factor.map_or(constraints.max_width, |f| child_size.width * f),
                self.height_factor.map_or(constraints.max_height, |f| child_size.height * f),
            );
            let size = constraints.constrain(size);
            
            self.base.align_child(child_size, size);
            self.base.inner.inner.size = size;
            size
        } else {
            constraints.biggest()
        }
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.base.paint_child(ctx, offset);
    }
}
```

---

## Пример 2: RenderPadding (Single child)

```rust
// ============================================================================
// RENDER PADDING - Single child + EdgeInsets
// ============================================================================

pub struct RenderPadding {
    base: ShiftedBoxBase,
    padding: EdgeInsets,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            base: ShiftedBoxBase::new(),
            padding,
        }
    }
    
    pub fn uniform(value: f32) -> Self {
        Self::new(EdgeInsets::all(value))
    }
    
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(horizontal, vertical))
    }
}

impl RenderBox for RenderPadding {
    type Arity = Single;
    
    fn perform_layout(&mut self, constraints: &BoxConstraints, layout: &mut LayoutHelper) -> Size {
        let child = self.base.child().expect("RenderPadding requires child");
        
        // Deflate constraints by padding
        let inner_constraints = constraints.deflate(&self.padding);
        let child_size = layout.layout_child(child, inner_constraints);
        
        // Position child at padding offset
        self.base.child_offset = Offset::new(self.padding.left, self.padding.top);
        
        // Our size = child + padding
        let size = Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        );
        
        self.base.inner.size = constraints.constrain(size);
        self.base.inner.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.base.paint_child(ctx, offset);
    }
}
```

---

## Пример 3: RenderOpacity (Proxy - Single child)

```rust
// ============================================================================
// RENDER OPACITY - Proxy pattern (delegates to child)
// ============================================================================

pub struct RenderOpacity {
    base: ProxyBoxBase,
    opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        Self {
            base: ProxyBoxBase::new(),
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
    
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl RenderBox for RenderOpacity {
    type Arity = Single;
    
    fn perform_layout(&mut self, constraints: &BoxConstraints, layout: &mut LayoutHelper) -> Size {
        // Proxy: просто передаём constraints ребёнку
        let child = self.base.child().expect("RenderOpacity requires child");
        let size = layout.layout_child(child, constraints);
        self.base.inner.size = size;
        size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return; // Полностью прозрачный - не рисуем
        }
        
        if self.opacity == 1.0 {
            // Полностью непрозрачный - рисуем напрямую
            self.base.paint_child(ctx, offset);
        } else {
            // Частичная прозрачность - через layer
            ctx.push_opacity(self.opacity, offset, |ctx| {
                self.base.paint_child(ctx, Offset::ZERO);
            });
        }
    }
}
```

---

## Пример 4: RenderFlex (Variable children)

```rust
// ============================================================================
// RENDER FLEX - Variable children (Column, Row)
// ============================================================================

pub struct RenderFlex {
    base: ContainerBoxBase,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    
    // Кэш позиций детей для paint
    child_offsets: Vec<Offset>,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            base: ContainerBoxBase::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            child_offsets: Vec::new(),
        }
    }
    
    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }
    
    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }
    
    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }
    
    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }
}

impl RenderBox for RenderFlex {
    type Arity = Variable;
    
    fn perform_layout(&mut self, constraints: &BoxConstraints, layout: &mut LayoutHelper) -> Size {
        let mut total_main_axis = 0.0;
        let mut max_cross_axis = 0.0;
        
        self.child_offsets.clear();
        
        // Phase 1: Layout non-flex children
        for child in self.base.children() {
            let child_constraints = match self.direction {
                Axis::Horizontal => BoxConstraints::new(
                    0.0, f32::INFINITY,
                    0.0, constraints.max_height,
                ),
                Axis::Vertical => BoxConstraints::new(
                    0.0, constraints.max_width,
                    0.0, f32::INFINITY,
                ),
            };
            
            let child_size = layout.layout_child(child, child_constraints);
            
            // Accumulate sizes
            match self.direction {
                Axis::Horizontal => {
                    total_main_axis += child_size.width;
                    max_cross_axis = max_cross_axis.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main_axis += child_size.height;
                    max_cross_axis = max_cross_axis.max(child_size.width);
                }
            }
        }
        
        // Phase 2: Position children
        let mut main_offset = 0.0;
        
        for child in self.base.children() {
            let child_size = layout.child_size(child);
            
            let cross_offset = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::Center => (max_cross_axis - child_size.cross(self.direction)) / 2.0,
                CrossAxisAlignment::End => max_cross_axis - child_size.cross(self.direction),
                _ => 0.0,
            };
            
            let offset = match self.direction {
                Axis::Horizontal => Offset::new(main_offset, cross_offset),
                Axis::Vertical => Offset::new(cross_offset, main_offset),
            };
            
            self.child_offsets.push(offset);
            main_offset += child_size.main(self.direction);
        }
        
        // Compute final size
        let size = match self.direction {
            Axis::Horizontal => Size::new(total_main_axis, max_cross_axis),
            Axis::Vertical => Size::new(max_cross_axis, total_main_axis),
        };
        
        self.base.inner.size = constraints.constrain(size);
        self.base.inner.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (i, child) in self.base.children().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);
            ctx.paint_child(child, offset + child_offset);
        }
    }
}
```

---

## Пример 5: RenderSliverList (Sliver protocol)

```rust
// ============================================================================
// SLIVER - Отдельный trait для Sliver protocol
// ============================================================================

pub trait RenderSliver: RenderObject {
    type Arity: Arity;
    
    fn perform_layout(
        &mut self,
        constraints: &SliverConstraints,
        layout: &mut LayoutHelper,
    ) -> SliverGeometry;
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
}

/// База для sliver с variable children
pub struct SliverListBase {
    pub inner: BoxBase<Variable>,
}

impl SliverListBase {
    pub fn new() -> Self {
        Self { inner: BoxBase::new() }
    }
    
    pub fn children(&self) -> impl Iterator<Item = RenderNodeId> + '_ {
        self.inner.children.iter()
    }
}

// ============================================================================
// RENDER SLIVER LIST
// ============================================================================

pub struct RenderSliverList {
    base: SliverListBase,
    item_extent: Option<f32>,  // fixed height per item (optimization)
    
    // Cache
    child_offsets: Vec<f32>,  // main axis offsets
}

impl RenderSliverList {
    pub fn new() -> Self {
        Self {
            base: SliverListBase::new(),
            item_extent: None,
            child_offsets: Vec::new(),
        }
    }
    
    pub fn with_fixed_extent(extent: f32) -> Self {
        Self {
            base: SliverListBase::new(),
            item_extent: Some(extent),
            child_offsets: Vec::new(),
        }
    }
}

impl RenderSliver for RenderSliverList {
    type Arity = Variable;
    
    fn perform_layout(
        &mut self,
        constraints: &SliverConstraints,
        layout: &mut LayoutHelper,
    ) -> SliverGeometry {
        self.child_offsets.clear();
        
        let mut scroll_offset = 0.0;
        let mut paint_extent = 0.0;
        
        for child in self.base.children() {
            // Layout child as Box
            let child_constraints = BoxConstraints::tight_for(
                Some(constraints.cross_axis_extent),
                self.item_extent,
            );
            
            let child_size = layout.layout_child(child, child_constraints);
            let child_extent = child_size.height; // assuming vertical
            
            self.child_offsets.push(scroll_offset);
            
            // Check if visible
            if scroll_offset + child_extent > constraints.scroll_offset &&
               scroll_offset < constraints.scroll_offset + constraints.remaining_paint_extent {
                paint_extent += child_extent;
            }
            
            scroll_offset += child_extent;
        }
        
        SliverGeometry {
            scroll_extent: scroll_offset,
            paint_extent: paint_extent.min(constraints.remaining_paint_extent),
            max_paint_extent: scroll_offset,
            ..Default::default()
        }
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (i, child) in self.base.children().enumerate() {
            let main_axis_offset = self.child_offsets.get(i).copied().unwrap_or(0.0);
            ctx.paint_child(child, offset + Offset::new(0.0, main_axis_offset));
        }
    }
}
```

---

## Сравнение Подходов

| Аспект | `RenderBox<A>` (generic param) | `RenderBox` + `type Arity` (associated) |
|--------|-------------------------------|----------------------------------------|
| Объявление trait | `trait RenderBox<A: Arity>` | `trait RenderBox { type Arity: Arity; }` |
| Impl | `impl RenderBox<Optional> for X` | `impl RenderBox for X { type Arity = Optional; }` |
| Хранение в коллекции | `Vec<Box<dyn RenderBox<??>>>` - сложно | `Vec<Box<dyn RenderBox>>` - просто |
| Arity виден | В сигнатуре типа | В associated type |

## Вывод

**Вариант 3** (Arity в base struct + associated type в trait):
- ✅ Простой `impl RenderBox for X`
- ✅ Легко хранить в коллекциях
- ✅ Base структуры переиспользуют код
- ✅ Конструкторы простые: `RenderAlign::new(alignment)`
- ✅ Один trait для Box, один для Sliver
