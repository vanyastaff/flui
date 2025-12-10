# RenderHandle + Deref Pattern

Дети хранятся внутри RenderObject как `RenderHandle<Mounted>` (как Flutter).
Через `Deref` получаем чистый API без boilerplate.

## RenderHandle с Deref

```rust
use std::ops::{Deref, DerefMut};
use flui_tree::{Mounted, Unmounted, NodeState, Depth};

/// Handle для render object с typestate
pub struct RenderHandle<S: NodeState> {
    render_object: Box<dyn RenderObject>,
    depth: Depth,
    parent: Option<RenderId>,
    _state: PhantomData<S>,
}

// Deref к RenderObject — вызываем методы напрямую!
impl<S: NodeState> Deref for RenderHandle<S> {
    type Target = dyn RenderObject;
    fn deref(&self) -> &Self::Target {
        self.render_object.as_ref()
    }
}

impl<S: NodeState> DerefMut for RenderHandle<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.render_object.as_mut()
    }
}

impl RenderHandle<Unmounted> {
    pub fn new<R: RenderObject + 'static>(render_object: R) -> Self {
        Self {
            render_object: Box::new(render_object),
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }
    
    pub fn mount(self, parent: Option<RenderId>, depth: Depth) -> RenderHandle<Mounted> {
        RenderHandle {
            render_object: self.render_object,
            depth,
            parent,
            _state: PhantomData,
        }
    }
}

impl RenderHandle<Mounted> {
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }
    
    pub fn depth(&self) -> Depth {
        self.depth
    }
    
    pub fn unmount(self) -> RenderHandle<Unmounted> {
        RenderHandle {
            render_object: self.render_object,
            depth: Depth::root(),
            parent: None,
            _state: PhantomData,
        }
    }
}
```

## Type Aliases

```rust
/// Single child (Optional)
pub type Child = Option<RenderHandle<Mounted>>;

/// Multiple children
pub type Children = Vec<RenderHandle<Mounted>>;
```

---

## Base Structs с Deref

Иерархия через композицию + `Deref`:

```
SingleChildBase
     ↑ Deref
ShiftedBoxBase (+ offset, size)
     ↑ Deref
RenderPadding (+ padding)
```

### SingleChildBase

```rust
/// Base для single child render objects (Flutter's RenderObjectWithChildMixin)
pub struct SingleChildBase {
    child: Child,  // Option<RenderHandle<Mounted>>
}

impl SingleChildBase {
    pub fn new() -> Self {
        Self { child: None }
    }
    
    pub fn with_child(child: RenderHandle<Mounted>) -> Self {
        Self { child: Some(child) }
    }
    
    pub fn child(&self) -> Option<&RenderHandle<Mounted>> {
        self.child.as_ref()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut RenderHandle<Mounted>> {
        self.child.as_mut()
    }
    
    pub fn set_child(&mut self, child: Child) {
        self.child = child;
    }
    
    pub fn take_child(&mut self) -> Child {
        self.child.take()
    }
}

impl Default for SingleChildBase {
    fn default() -> Self {
        Self::new()
    }
}
```

### ShiftedBoxBase

```rust
/// Base для shifted box (single child + offset + size)
/// Flutter's RenderShiftedBox
pub struct ShiftedBoxBase {
    base: SingleChildBase,
    pub child_offset: Offset,
    pub size: Size,
}

impl ShiftedBoxBase {
    pub fn new() -> Self {
        Self {
            base: SingleChildBase::new(),
            child_offset: Offset::ZERO,
            size: Size::ZERO,
        }
    }
}

impl Default for ShiftedBoxBase {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for ShiftedBoxBase {
    type Target = SingleChildBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for ShiftedBoxBase {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}
```

### AligningBoxBase

```rust
/// Base для aligning box (shifted + alignment)
/// Flutter's RenderAligningShiftedBox
pub struct AligningBoxBase {
    base: ShiftedBoxBase,
    pub alignment: Alignment,
}

impl AligningBoxBase {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: ShiftedBoxBase::new(),
            alignment,
        }
    }
    
    pub fn align_child(&mut self, child_size: Size, container_size: Size) {
        self.base.child_offset = self.alignment.compute_offset(child_size, container_size);
    }
}

impl Deref for AligningBoxBase {
    type Target = ShiftedBoxBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for AligningBoxBase {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}
```

### ProxyBoxBase

```rust
/// Base для proxy box (делегирует всё ребёнку)
/// Flutter's RenderProxyBox
pub struct ProxyBoxBase {
    base: SingleChildBase,
    pub size: Size,
}

impl ProxyBoxBase {
    pub fn new() -> Self {
        Self {
            base: SingleChildBase::new(),
            size: Size::ZERO,
        }
    }
}

impl Deref for ProxyBoxBase {
    type Target = SingleChildBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for ProxyBoxBase {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}
```

### ContainerBase

```rust
/// Base для multiple children (Flutter's ContainerRenderObjectMixin)
pub struct ContainerBase {
    children: Children,  // Vec<RenderHandle<Mounted>>
    pub size: Size,
}

impl ContainerBase {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            size: Size::ZERO,
        }
    }
    
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            children: Vec::with_capacity(capacity),
            size: Size::ZERO,
        }
    }
    
    pub fn children(&self) -> &[RenderHandle<Mounted>] {
        &self.children
    }
    
    pub fn children_mut(&mut self) -> &mut Vec<RenderHandle<Mounted>> {
        &mut self.children
    }
    
    pub fn add(&mut self, child: RenderHandle<Mounted>) {
        self.children.push(child);
    }
    
    pub fn insert(&mut self, index: usize, child: RenderHandle<Mounted>) {
        self.children.insert(index, child);
    }
    
    pub fn remove(&mut self, index: usize) -> RenderHandle<Mounted> {
        self.children.remove(index)
    }
    
    pub fn clear(&mut self) {
        self.children.clear();
    }
    
    pub fn len(&self) -> usize {
        self.children.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
    
    pub fn iter(&self) -> impl Iterator<Item = &RenderHandle<Mounted>> {
        self.children.iter()
    }
    
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut RenderHandle<Mounted>> {
        self.children.iter_mut()
    }
}

impl Default for ContainerBase {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Traits (без LayoutHelper!)

```rust
/// Base trait для всех render objects
pub trait RenderObject: Send + Sync + 'static {
    fn debug_name(&self) -> &'static str;
    
    // ... другие базовые методы
}

/// Box protocol
pub trait RenderBox: RenderObject {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool;
    
    // Intrinsics (optional)
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
}

/// Sliver protocol
pub trait RenderSliver: RenderObject {
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    fn hit_test(&self, result: &mut SliverHitTestResult, position: SliverOffset) -> bool;
}
```

---

## Примеры

### RenderView (root)

```rust
pub struct RenderView {
    base: SingleChildBase,
    pub configuration: ViewConfiguration,
}

impl RenderView {
    pub fn new(configuration: ViewConfiguration) -> Self {
        Self {
            base: SingleChildBase::new(),
            configuration,
        }
    }
}

impl Deref for RenderView {
    type Target = SingleChildBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderView {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderView {
    pub fn layout(&mut self) {
        let constraints = BoxConstraints::tight(self.configuration.size);
        if let Some(child) = self.child_mut() {
            // Deref на RenderHandle → вызываем perform_layout напрямую!
            child.perform_layout(&constraints);
        }
    }
    
    pub fn paint(&self, ctx: &mut PaintingContext) {
        if let Some(child) = self.child() {
            child.paint(ctx, Offset::ZERO);
        }
    }
}
```

### RenderPadding

```rust
pub struct RenderPadding {
    base: ShiftedBoxBase,
    pub padding: EdgeInsets,
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
}

impl Deref for RenderPadding {
    type Target = ShiftedBoxBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderPadding {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            let inner = constraints.deflate(&self.padding);
            
            // Напрямую через Deref!
            let child_size = child.perform_layout(&inner);
            
            self.child_offset = Offset::new(self.padding.left, self.padding.top);
            self.size = Size::new(
                child_size.width + self.padding.horizontal(),
                child_size.height + self.padding.vertical(),
            );
        } else {
            self.size = constraints.smallest();
        }
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            child.paint(ctx, offset + self.child_offset);
        }
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            let child_position = position - self.child_offset;
            child.hit_test(result, child_position)
        } else {
            false
        }
    }
}
```

### RenderAlign

```rust
pub struct RenderAlign {
    base: AligningBoxBase,
    pub width_factor: Option<f32>,
    pub height_factor: Option<f32>,
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
}

impl Deref for RenderAlign {
    type Target = AligningBoxBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderAlign {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderBox for RenderAlign {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            let child_size = child.perform_layout(&constraints.loosen());
            
            self.size = Size::new(
                self.width_factor.map_or(constraints.max_width, |f| child_size.width * f),
                self.height_factor.map_or(constraints.max_height, |f| child_size.height * f),
            );
            self.size = constraints.constrain(self.size);
            
            self.align_child(child_size, self.size);
        } else {
            self.size = constraints.biggest();
        }
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            child.paint(ctx, offset + self.child_offset);
        }
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position - self.child_offset)
        } else {
            false
        }
    }
}
```

### RenderOpacity (Proxy)

```rust
pub struct RenderOpacity {
    base: ProxyBoxBase,
    pub opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        Self {
            base: ProxyBoxBase::new(),
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
}

impl Deref for RenderOpacity {
    type Target = ProxyBoxBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderOpacity {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            self.size = child.perform_layout(constraints);
        } else {
            self.size = constraints.smallest();
        }
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return;
        }
        
        if let Some(child) = self.child() {
            if self.opacity == 1.0 {
                child.paint(ctx, offset);
            } else {
                ctx.push_opacity(self.opacity, offset, |ctx| {
                    child.paint(ctx, Offset::ZERO);
                });
            }
        }
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }
}
```

### RenderFlex (Container)

```rust
pub struct RenderFlex {
    base: ContainerBase,
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
    
    // Cache
    child_offsets: Vec<Offset>,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            base: ContainerBase::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            child_offsets: Vec::new(),
        }
    }
    
    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }
    
    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }
}

impl Deref for RenderFlex {
    type Target = ContainerBase;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderFlex {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        self.child_offsets.clear();
        
        let mut total_main = 0.0;
        let mut max_cross = 0.0;
        let mut child_sizes = Vec::new();
        
        // Phase 1: Layout children
        for child in self.children_mut() {
            let child_constraints = match self.direction {
                Axis::Horizontal => BoxConstraints::new(0.0, f32::INFINITY, 0.0, constraints.max_height),
                Axis::Vertical => BoxConstraints::new(0.0, constraints.max_width, 0.0, f32::INFINITY),
            };
            
            let child_size = child.perform_layout(&child_constraints);
            child_sizes.push(child_size);
            
            match self.direction {
                Axis::Horizontal => {
                    total_main += child_size.width;
                    max_cross = max_cross.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main += child_size.height;
                    max_cross = max_cross.max(child_size.width);
                }
            }
        }
        
        // Phase 2: Position children
        let mut main_offset = 0.0;
        for child_size in &child_sizes {
            let cross_offset = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::Center => {
                    let child_cross = match self.direction {
                        Axis::Horizontal => child_size.height,
                        Axis::Vertical => child_size.width,
                    };
                    (max_cross - child_cross) / 2.0
                }
                CrossAxisAlignment::End => {
                    let child_cross = match self.direction {
                        Axis::Horizontal => child_size.height,
                        Axis::Vertical => child_size.width,
                    };
                    max_cross - child_cross
                }
                _ => 0.0,
            };
            
            let offset = match self.direction {
                Axis::Horizontal => Offset::new(main_offset, cross_offset),
                Axis::Vertical => Offset::new(cross_offset, main_offset),
            };
            self.child_offsets.push(offset);
            
            main_offset += match self.direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            };
        }
        
        self.size = match self.direction {
            Axis::Horizontal => constraints.constrain(Size::new(total_main, max_cross)),
            Axis::Vertical => constraints.constrain(Size::new(max_cross, total_main)),
        };
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (i, child) in self.children().iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);
            child.paint(ctx, offset + child_offset);
        }
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        for (i, child) in self.children().iter().enumerate().rev() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);
            if child.hit_test(result, position - child_offset) {
                return true;
            }
        }
        false
    }
}
```

---

## Сравнение с Flutter

| Flutter | Rust FLUI |
|---------|-----------|
| `child!.layout(constraints)` | `child.perform_layout(&constraints)` |
| `child!.paint(context, offset)` | `child.paint(ctx, offset)` |
| `class RenderPadding extends RenderShiftedBox` | `struct RenderPadding { base: ShiftedBoxBase }` + `Deref` |
| `RenderObjectWithChildMixin` | `SingleChildBase` |
| `ContainerRenderObjectMixin` | `ContainerBase` |

## Преимущества

- ✅ **Чистый API** — `child.perform_layout()` напрямую через Deref
- ✅ **Нет boilerplate** — base structs с Deref дают методы бесплатно
- ✅ **Как Flutter** — дети внутри RenderObject, не в отдельном tree
- ✅ **Нет LayoutHelper** — layout напрямую на детях
- ✅ **Typestate** — `RenderHandle<Unmounted>` / `RenderHandle<Mounted>`
- ✅ **Композиция** — иерархия через Deref вместо наследования
