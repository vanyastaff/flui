# FLUI Arity System: Compile-Time Child Count Validation

## Overview

The arity system is a **key innovation** in FLUI that provides compile-time guarantees about child count, preventing entire classes of runtime errors.

**Core Idea:** Use Rust's type system to enforce child count constraints at compile time.

---

## Current Arity System (Good Foundation!)

### Type-Level Arity Markers

```rust
// Zero-sized marker types
pub struct Leaf;       // 0 children
pub struct Single;     // Exactly 1 child
pub struct Optional;   // 0 or 1 child
pub struct Variable;   // 0+ children
```

### RenderObject with Arity

```rust
pub trait RenderBox<A: Arity> {
    fn layout(&mut self, ctx: LayoutContext<'_, A>) -> Size;
    fn paint(&self, ctx: &PaintContext<'_, A>);
}

// Leaf node - no children
impl RenderBox<Leaf> for RenderRectangle {
    fn layout(&mut self, ctx: LayoutContext<'_, Leaf>) -> Size {
        // ctx.children() is empty - enforced by type!
        ctx.constraints.biggest()
    }
}

// Single child
impl RenderBox<Single> for RenderPadding {
    fn layout(&mut self, ctx: LayoutContext<'_, Single>) -> Size {
        let child_id = ctx.children().single();  // Type-safe!
        // ...
    }
}
```

---

## Improvements with Rust 1.91

### 1. Const Generics for Fixed Arity

**For cases where you know exact count:**

```rust
// Exact count with const generic
pub struct Exact<const N: usize>;

// Arity aliases
pub type Leaf = Exact<0>;
pub type Single = Exact<1>;
pub type Double = Exact<2>;
pub type Triple = Exact<3>;

impl<const N: usize> Arity for Exact<N> {
    type Storage = [ElementId; N];  // Fixed-size array!

    fn validate_count(count: usize) -> bool {
        count == N
    }
}

// Usage
pub struct RenderRow<const N: usize> {
    children_sizes: [Size; N],  // Fixed-size!
}

impl<const N: usize> RenderBox<Exact<N>> for RenderRow<N> {
    fn layout(&mut self, ctx: LayoutContext<'_, Exact<N>>) -> Size {
        // Children access is &[ElementId; N] - no Vec!
        let children = ctx.children().array();

        // Can iterate with known compile-time size
        for (i, &child_id) in children.iter().enumerate() {
            self.children_sizes[i] = ctx.layout_child(child_id, constraints)?;
        }

        // ...
    }
}
```

**Benefits:**
- ✅ Stack allocation (no heap)
- ✅ No bounds checking
- ✅ Perfect for small fixed counts

### 2. Type-Safe Children Access

```rust
/// Context children view with arity-specific methods
pub struct ChildrenView<'ctx, A: Arity> {
    element_id: ElementId,
    element_tree: &'ctx ElementTree,
    _arity: PhantomData<A>,
}

// Leaf (0 children) - compile error if you try to access!
impl<'ctx> ChildrenView<'ctx, Leaf> {
    // No child access methods - can't access what doesn't exist!
}

// Single (1 child)
impl<'ctx> ChildrenView<'ctx, Single> {
    pub fn single(&self) -> ElementId {
        let element = self.element_tree.get(self.element_id).unwrap();
        element.children()[0]  // Always safe - arity guarantees 1 child
    }

    pub fn get(&self) -> ElementId {
        self.single()
    }
}

// Optional (0 or 1)
impl<'ctx> ChildrenView<'ctx, Optional> {
    pub fn get(&self) -> Option<ElementId> {
        let element = self.element_tree.get(self.element_id).unwrap();
        element.children().first().copied()
    }

    pub fn is_some(&self) -> bool {
        self.get().is_some()
    }

    pub fn is_none(&self) -> bool {
        self.get().is_none()
    }
}

// Variable (0+)
impl<'ctx> ChildrenView<'ctx, Variable> {
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + 'ctx {
        let element = self.element_tree.get(self.element_id).unwrap();
        element.children().iter().copied()
    }

    pub fn len(&self) -> usize {
        let element = self.element_tree.get(self.element_id).unwrap();
        element.children().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<ElementId> {
        let element = self.element_tree.get(self.element_id).unwrap();
        element.children().get(index).copied()
    }

    pub fn first(&self) -> Option<ElementId> {
        self.get(0)
    }

    pub fn last(&self) -> Option<ElementId> {
        let len = self.len();
        if len > 0 {
            self.get(len - 1)
        } else {
            None
        }
    }
}

// Exact<N> (N children)
impl<'ctx, const N: usize> ChildrenView<'ctx, Exact<N>> {
    pub fn array(&self) -> &[ElementId; N] {
        let element = self.element_tree.get(self.element_id).unwrap();
        // Safe because arity guarantees exactly N children
        element.children()[..N].try_into().unwrap()
    }

    pub fn iter(&self) -> impl Iterator<Item = ElementId> + 'ctx {
        self.array().iter().copied()
    }

    pub fn get(&self, index: usize) -> ElementId {
        debug_assert!(index < N);
        self.array()[index]
    }
}
```

### 3. Context with Arity

```rust
pub struct LayoutContext<'tree, A: Arity, P: Protocol = BoxProtocol> {
    element_tree: &'tree ElementTree,
    element_id: ElementId,
    constraints: P::Constraints,
    cache: &'tree mut LayoutCache,
    _arity: PhantomData<A>,
    _protocol: PhantomData<P>,
}

impl<'tree, A: Arity, P: Protocol> LayoutContext<'tree, A, P> {
    /// Get type-safe children view
    pub fn children(&self) -> ChildrenView<'_, A> {
        ChildrenView {
            element_id: self.element_id,
            element_tree: self.element_tree,
            _arity: PhantomData,
        }
    }

    /// Layout child with type-checked access
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: P::Constraints,
    ) -> Result<P::Geometry, LayoutError> {
        // ... implementation
    }
}
```

---

## Complete Example: RenderFlex with Arity

```rust
pub struct RenderFlex {
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    // Child data
    child_sizes: Vec<Size>,
    child_positions: Vec<Offset>,
}

impl RenderBox<Variable> for RenderFlex {
    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, Variable, BoxProtocol>,
    ) -> Result<Size, LayoutError> {
        // Type-safe: Variable arity means we have iter()
        let child_count = ctx.children().len();

        // Prepare storage
        self.child_sizes.clear();
        self.child_sizes.reserve(child_count);

        let mut total_main_size = 0.0;
        let mut max_cross_size = 0.0;

        // Iterate children (type-safe!)
        for child_id in ctx.children().iter() {
            // Get flex data from parent data
            let flex = ctx.parent_data::<FlexParentData>(child_id)
                .map(|pd| pd.flex)
                .unwrap_or(0);

            // Layout child
            let child_constraints = if flex > 0 {
                // Flexible child
                match self.direction {
                    Axis::Horizontal => BoxConstraints::new(
                        0.0, f32::INFINITY,
                        ctx.constraints.min_height, ctx.constraints.max_height,
                    ),
                    Axis::Vertical => BoxConstraints::new(
                        ctx.constraints.min_width, ctx.constraints.max_width,
                        0.0, f32::INFINITY,
                    ),
                }
            } else {
                // Inflexible child
                ctx.constraints.loosen()
            };

            let child_size = ctx.layout_child(child_id, child_constraints)?;
            self.child_sizes.push(child_size);

            // Update sizes
            match self.direction {
                Axis::Horizontal => {
                    total_main_size += child_size.width;
                    max_cross_size = max_cross_size.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main_size += child_size.height;
                    max_cross_size = max_cross_size.max(child_size.width);
                }
            }
        }

        // Calculate positions based on alignment
        self.calculate_positions(
            total_main_size,
            max_cross_size,
            ctx.constraints,
        );

        // Return size
        Ok(match self.direction {
            Axis::Horizontal => Size::new(total_main_size, max_cross_size),
            Axis::Vertical => Size::new(max_cross_size, total_main_size),
        })
    }

    fn paint(
        &self,
        ctx: &mut PaintContext<'_, Variable>,
    ) -> Result<(), PaintError> {
        // Paint each child at calculated position
        for (i, child_id) in ctx.children().iter().enumerate() {
            let offset = self.child_positions[i];
            ctx.paint_child(child_id, offset)?;
        }
        Ok(())
    }
}
```

---

## Arity in Element Construction

### Builder Pattern with Arity

```rust
pub struct ElementBuilder<A: Arity> {
    children: Vec<ElementId>,
    _arity: PhantomData<A>,
}

impl<A: Arity> ElementBuilder<A> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            _arity: PhantomData,
        }
    }

    pub fn build(self) -> Result<Element, ArityError> {
        // Validate arity
        if !A::validate_count(self.children.len()) {
            return Err(ArityError::InvalidCount {
                expected: A::describe(),
                actual: self.children.len(),
            });
        }

        Ok(Element {
            children: self.children,
            // ...
        })
    }
}

// Specialized for Single
impl ElementBuilder<Single> {
    pub fn child(mut self, child: ElementId) -> Self {
        self.children.push(child);
        self
    }

    // Type-safe build - guarantees exactly 1 child
    pub fn build_single(self) -> Result<Element, ArityError> {
        if self.children.len() != 1 {
            return Err(ArityError::NotSingle);
        }
        self.build()
    }
}

// Specialized for Variable
impl ElementBuilder<Variable> {
    pub fn add_child(mut self, child: ElementId) -> Self {
        self.children.push(child);
        self
    }

    pub fn add_children(mut self, children: impl IntoIterator<Item = ElementId>) -> Self {
        self.children.extend(children);
        self
    }
}

// Usage
let element = ElementBuilder::<Single>::new()
    .child(child_id)
    .build_single()?;
```

---

## Advanced: Range-Based Arity

```rust
pub struct Range<const MIN: usize, const MAX: usize>;

pub type AtLeast<const N: usize> = Range<N, { usize::MAX }>;
pub type AtMost<const N: usize> = Range<0, N>;

impl<const MIN: usize, const MAX: usize> Arity for Range<MIN, MAX> {
    type Storage = Vec<ElementId>;

    fn validate_count(count: usize) -> bool {
        count >= MIN && count <= MAX
    }

    fn describe() -> String {
        if MIN == MAX {
            format!("exactly {}", MIN)
        } else if MAX == usize::MAX {
            format!("at least {}", MIN)
        } else {
            format!("{} to {}", MIN, MAX)
        }
    }
}

// Usage
pub type TwoToFour = Range<2, 4>;

impl RenderBox<TwoToFour> for RenderCustom {
    fn layout(&mut self, ctx: &mut LayoutContext<'_, TwoToFour>) -> Size {
        // Guaranteed to have 2-4 children
        let children = ctx.children().iter().collect::<Vec<_>>();
        assert!(children.len() >= 2 && children.len() <= 4);
        // ...
    }
}
```

---

## Integration with 4-Tree Architecture

### ViewTree with Arity

```rust
pub trait View {
    type Arity: Arity;

    fn build<'ctx>(
        &self,
        ctx: &BuildContext<'ctx>,
    ) -> Element<Self::Arity>;
}

pub struct PaddingView {
    padding: EdgeInsets,
    child: Box<dyn View<Arity = impl Arity>>,
}

impl View for PaddingView {
    type Arity = Single;  // Padding always has 1 child

    fn build<'ctx>(&self, ctx: &BuildContext<'ctx>) -> Element<Single> {
        let child_element = self.child.build(ctx);

        Element::new(
            RenderPadding::new(self.padding),
            vec![child_element.id()],
        )
    }
}
```

### ElementTree with Arity Validation

```rust
pub struct Element {
    id: ElementId,
    children: Vec<ElementId>,
    render_object: Box<dyn RenderObject>,
    arity: RuntimeArity,  // Runtime arity for validation
}

pub enum RuntimeArity {
    Leaf,
    Single,
    Optional,
    Variable,
    Exact(usize),
    Range { min: usize, max: usize },
}

impl Element {
    /// Validate children count matches arity
    pub fn validate_arity(&self) -> Result<(), ArityError> {
        let count = self.children.len();

        match self.arity {
            RuntimeArity::Leaf => {
                if count != 0 {
                    return Err(ArityError::ExpectedLeaf { actual: count });
                }
            }
            RuntimeArity::Single => {
                if count != 1 {
                    return Err(ArityError::ExpectedSingle { actual: count });
                }
            }
            RuntimeArity::Optional => {
                if count > 1 {
                    return Err(ArityError::ExpectedOptional { actual: count });
                }
            }
            RuntimeArity::Variable => {
                // Any count is valid
            }
            RuntimeArity::Exact(n) => {
                if count != n {
                    return Err(ArityError::ExpectedExact { expected: n, actual: count });
                }
            }
            RuntimeArity::Range { min, max } => {
                if count < min || count > max {
                    return Err(ArityError::ExpectedRange {
                        min,
                        max,
                        actual: count,
                    });
                }
            }
        }

        Ok(())
    }
}
```

---

## Error Messages with Arity

```rust
pub enum ArityError {
    ExpectedLeaf { actual: usize },
    ExpectedSingle { actual: usize },
    ExpectedOptional { actual: usize },
    ExpectedExact { expected: usize, actual: usize },
    ExpectedRange { min: usize, max: usize, actual: usize },
}

impl std::fmt::Display for ArityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExpectedLeaf { actual } => {
                write!(f, "Expected leaf node (0 children), got {}", actual)
            }
            Self::ExpectedSingle { actual } => {
                write!(f, "Expected single child (1 child), got {}", actual)
            }
            Self::ExpectedOptional { actual } => {
                write!(f, "Expected optional child (0-1 children), got {}", actual)
            }
            Self::ExpectedExact { expected, actual } => {
                write!(f, "Expected exactly {} children, got {}", expected, actual)
            }
            Self::ExpectedRange { min, max, actual } => {
                write!(f, "Expected {}-{} children, got {}", min, max, actual)
            }
        }
    }
}
```

---

## Benefits Summary

### Compile-Time Safety
- ✅ **Can't access children that don't exist** (Leaf has no child methods)
- ✅ **Can't forget to handle optional child** (Optional returns Option<ElementId>)
- ✅ **Can't assume single child exists** (must use type-safe accessor)

### Performance
- ✅ **No runtime bounds checking** with const generics
- ✅ **Stack allocation** for fixed arity (Exact<N>)
- ✅ **Better optimizations** - compiler knows exact counts

### Developer Experience
- ✅ **Clear API contracts** - function signature shows arity
- ✅ **Better IDE support** - autocomplete knows available methods
- ✅ **Helpful error messages** - arity mismatch caught early

### Flexibility
- ✅ **Works with any arity** - Leaf, Single, Optional, Variable, Exact<N>, Range<MIN, MAX>
- ✅ **Extensible** - can define custom arity types
- ✅ **Composable** - arity works with protocols, contexts, etc.

---

## Conclusion

The arity system is a **killer feature** of FLUI! It should:

1. ✅ **Be preserved** in the new architecture
2. ✅ **Be enhanced** with const generics (Rust 1.51+)
3. ✅ **Integrate deeply** with context-based API
4. ✅ **Provide type-safe** child access patterns

This makes FLUI's type safety **better than Flutter**! 🚀
