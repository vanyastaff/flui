# FLUI Unified RenderObject Architecture

## Technical Specification v1.5

**Revision Notes v1.5:**
- Added `strict-arity` feature flag for release-mode validation
- ElementId in all error messages for debugging
- `#[inline(always)]` on all `from_slice` implementations
- Added loom deadlock prevention test example
- Added benchmark examples for debug_assert zero-cost verification
- Documented when to use transactional vs atomic updates
- Enhanced safety documentation for `remove_child`

**Previous Revisions:**
- v1.4: Source of truth fix, debug_assert, transactional API, scheduling
- v1.3: Added `Optional` arity type, `OptionalChild` accessor
- v1.2: Public traits, downcast-rs, dyn-clone, removed macros
- v1.1: Removed unsafe, added arity enforcement, dual-path migration

---

## Executive Summary

This document specifies the migration from FLUI's current dual-element architecture (RenderElement + SliverElement) to a unified, type-safe RenderObject system.

### Key Design Decisions

1. **Public traits**: `Render<A>` and `SliverRender<A>` for excellent DX
2. **Internal trait**: `RenderObject<P, A>` hidden from users
3. **downcast-rs**: Industry-standard safe downcasting
4. **dyn-clone**: Trait object cloning when needed
5. **No macros**: Pure idiomatic Rust
6. **Optional arity**: First-class support for 0-or-1 child widgets
7. **Single source of truth**: protocol/arity stored only in RenderElement
8. **Transactional updates**: Safe batch children modifications

### Acceptance Criteria

- [ ] All render objects migrated or behind feature flag
- [ ] Benchmarks show ≤10% perf regression
- [ ] No unsafe in wrapper creation
- [ ] Documentation updated
- [ ] Arity violation triggers descriptive panic (debug only in hot-path)

---

## Table of Contents

1. [Dependencies](#1-dependencies)
2. [Architecture Overview](#2-architecture-overview)
3. [Protocol System](#3-protocol-system)
4. [Arity System](#4-arity-system)
5. [Public Traits (Render/SliverRender)](#5-public-traits)
6. [Internal RenderObject Trait](#6-internal-renderobject-trait)
7. [Context Types](#7-context-types)
8. [RenderState](#8-renderstate)
9. [Type Erasure Layer](#9-type-erasure-layer)
10. [RenderElement](#10-renderelement)
11. [Element Enum](#11-element-enum)
12. [ElementTree Integration](#12-elementtree-integration)
13. [Thread Safety Guarantees](#13-thread-safety-guarantees)
14. [Usage Examples](#14-usage-examples)
15. [Migration Guide](#15-migration-guide)
16. [Implementation Plan](#16-implementation-plan)
17. [Testing Strategy](#17-testing-strategy)
18. [Performance Considerations](#18-performance-considerations)
19. [API Reference](#19-api-reference)

---

## 1. Dependencies

### Cargo.toml

```toml
[dependencies]
# Safe downcasting for trait objects
downcast-rs = "1.2"

# Cloning trait objects (optional)
dyn-clone = "1.0"

# Existing dependencies
parking_lot = "0.12"
flui_types = { path = "../flui_types" }
flui_painting = { path = "../flui_painting" }

[features]
default = []
# Enable strict arity validation even in release builds (for debugging)
strict-arity = []
# Enable parallel layout (requires rayon)
parallel = ["rayon"]
```

---

## 2. Architecture Overview

### Arity Hierarchy

```
Arity Types:
├── Leaf            → 0 children       (Text, Image, Spacer)
├── Optional        → 0 or 1 child     (SizedBox, Container, ColoredBox)
├── Single          → exactly 1 child  (Padding, Align, Center)
├── Pair            → exactly 2 children
├── Triple          → exactly 3 children
├── Exact<N>        → exactly N children
├── AtLeast<N>      → N or more children
└── Variable        → any number       (Flex, Column, Row)
```

### Storage vs Access

| Aspect | Implementation |
|--------|----------------|
| **Storage** | Always `Vec<ElementId>` in RenderElement |
| **Validation** | Arity enforces count on add/remove (strict in mutation, debug in hot-path) |
| **Access API** | Typed accessor per arity (single(), get(), iter()) |
| **Source of Truth** | `protocol` and `arity` stored ONLY in RenderElement fields |

---

## 3. Protocol System

### 3.1 Sealed Trait

```rust
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::BoxProtocol {}
    impl Sealed for super::SliverProtocol {}
}
```

### 3.2 Protocol Trait

```rust
pub trait Protocol: sealed::Sealed + Send + Sync + 'static {
    type Constraints: Clone + Debug + Default + Send + Sync;
    type Geometry: Clone + Debug + Default + Send + Sync;
    type LayoutContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type PaintContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type HitTestContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type HitTestResult: Debug + Default;
    
    const ID: LayoutProtocol;
    const NAME: &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutProtocol {
    Box,
    Sliver,
}
```

### 3.3 Protocol Implementations

```rust
#[derive(Debug, Clone, Copy)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type LayoutContext<'a, A: Arity> = BoxLayoutContext<'a, A>;
    type PaintContext<'a, A: Arity> = BoxPaintContext<'a, A>;
    type HitTestContext<'a, A: Arity> = BoxHitTestContext<'a, A>;
    type HitTestResult = bool;
    
    const ID: LayoutProtocol = LayoutProtocol::Box;
    const NAME: &'static str = "Box";
}

#[derive(Debug, Clone, Copy)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type LayoutContext<'a, A: Arity> = SliverLayoutContext<'a, A>;
    type PaintContext<'a, A: Arity> = SliverPaintContext<'a, A>;
    type HitTestContext<'a, A: Arity> = SliverHitTestContext<'a, A>;
    type HitTestResult = SliverHitTestResult;
    
    const ID: LayoutProtocol = LayoutProtocol::Sliver;
    const NAME: &'static str = "Sliver";
}
```

---

## 4. Arity System

### 4.1 Arity Trait

```rust
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Leaf {}
    impl Sealed for super::Optional {}
    impl<const N: usize> Sealed for super::Exact<N> {}
    impl<const N: usize> Sealed for super::AtLeast<N> {}
    impl Sealed for super::Variable {}
}

/// Compile-time arity specification
pub trait Arity: sealed::Sealed + Send + Sync + 'static {
    /// Children accessor type
    type Children<'a>: ChildrenAccess;
    
    /// Runtime arity info
    fn runtime_arity() -> RuntimeArity;
    
    /// Validate child count
    fn validate_count(count: usize) -> bool;
    
    /// Convert slice to typed accessor (panics if invalid)
    fn from_slice(children: &[ElementId]) -> Self::Children<'_>;
    
    /// Try to convert (returns None if invalid)
    fn try_from_slice(children: &[ElementId]) -> Option<Self::Children<'_>> {
        if Self::validate_count(children.len()) {
            Some(Self::from_slice(children))
        } else {
            None
        }
    }
}

/// Runtime arity information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArity {
    Exact(usize),
    AtLeast(usize),
    Optional,
    Variable,
}

impl RuntimeArity {
    pub fn validate(&self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == *n,
            Self::AtLeast(n) => count >= *n,
            Self::Optional => count <= 1,
            Self::Variable => true,
        }
    }
}

impl std::fmt::Display for RuntimeArity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(0) => write!(f, "no children (leaf)"),
            Self::Exact(1) => write!(f, "exactly 1 child"),
            Self::Exact(n) => write!(f, "exactly {} children", n),
            Self::AtLeast(n) => write!(f, "at least {} children", n),
            Self::Optional => write!(f, "0 or 1 child"),
            Self::Variable => write!(f, "any number of children"),
        }
    }
}
```

### 4.2 Arity Types

```rust
/// Leaf - 0 children
#[derive(Debug, Clone, Copy)]
pub struct Leaf;

impl Arity for Leaf {
    type Children<'a> = NoChildren;
    
    fn runtime_arity() -> RuntimeArity { RuntimeArity::Exact(0) }
    fn validate_count(count: usize) -> bool { count == 0 }
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> {
        debug_assert!(children.is_empty(), "Leaf expects 0 children, got {}", children.len());
        NoChildren
    }
}

/// Optional - 0 or 1 child
#[derive(Debug, Clone, Copy)]
pub struct Optional;

impl Arity for Optional {
    type Children<'a> = OptionalChild<'a>;
    
    fn runtime_arity() -> RuntimeArity { RuntimeArity::Optional }
    fn validate_count(count: usize) -> bool { count <= 1 }
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> {
        debug_assert!(
            children.len() <= 1,
            "Optional expects 0 or 1 child, got {}",
            children.len()
        );
        OptionalChild { children }
    }
}

/// Exact<N> - exactly N children
#[derive(Debug, Clone, Copy)]
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    type Children<'a> = FixedChildren<'a, N>;
    
    fn runtime_arity() -> RuntimeArity { RuntimeArity::Exact(N) }
    fn validate_count(count: usize) -> bool { count == N }
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> {
        debug_assert!(
            children.len() == N,
            "Exact<{}> expects {} children, got {}",
            N, N, children.len()
        );
        // Safe: we've validated the length
        let arr: &[ElementId; N] = children.try_into()
            .expect("slice length already validated");
        FixedChildren { children: arr }
    }
}

/// Type aliases
pub type Single = Exact<1>;
pub type Pair = Exact<2>;
pub type Triple = Exact<3>;

/// AtLeast<N> - N or more children
#[derive(Debug, Clone, Copy)]
pub struct AtLeast<const N: usize>;

impl<const N: usize> Arity for AtLeast<N> {
    type Children<'a> = SliceChildren<'a>;
    
    fn runtime_arity() -> RuntimeArity { RuntimeArity::AtLeast(N) }
    fn validate_count(count: usize) -> bool { count >= N }
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> {
        debug_assert!(
            children.len() >= N,
            "AtLeast<{}> expects >= {} children, got {}",
            N, N, children.len()
        );
        SliceChildren { children }
    }
}

/// Variable - any number
#[derive(Debug, Clone, Copy)]
pub struct Variable;

impl Arity for Variable {
    type Children<'a> = SliceChildren<'a>;
    
    fn runtime_arity() -> RuntimeArity { RuntimeArity::Variable }
    fn validate_count(_: usize) -> bool { true }
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> { SliceChildren { children } }
}
```

### 4.3 Children Accessors

```rust
/// Trait for children access
pub trait ChildrenAccess: Debug + Copy {
    fn as_slice(&self) -> &[ElementId];
    fn len(&self) -> usize { self.as_slice().len() }
    fn is_empty(&self) -> bool { self.as_slice().is_empty() }
}

/// No children (for Leaf)
#[derive(Debug, Clone, Copy)]
pub struct NoChildren;

impl ChildrenAccess for NoChildren {
    fn as_slice(&self) -> &[ElementId] { &[] }
}

/// Optional child (for Optional arity)
#[derive(Debug, Clone, Copy)]
pub struct OptionalChild<'a> {
    children: &'a [ElementId],
}

impl ChildrenAccess for OptionalChild<'_> {
    fn as_slice(&self) -> &[ElementId] { self.children }
}

impl<'a> OptionalChild<'a> {
    /// Get the optional child
    #[inline(always)]
    pub fn get(&self) -> Option<ElementId> {
        self.children.first().copied()
    }
    
    /// Check if child exists
    #[inline(always)]
    pub fn is_some(&self) -> bool {
        !self.children.is_empty()
    }
    
    /// Check if no child
    #[inline(always)]
    pub fn is_none(&self) -> bool {
        self.children.is_empty()
    }
    
    /// Get child or panic
    #[inline(always)]
    pub fn unwrap(&self) -> ElementId {
        self.children.first().copied().expect("Optional child is None")
    }
    
    /// Get child or default
    #[inline(always)]
    pub fn unwrap_or(&self, default: ElementId) -> ElementId {
        self.children.first().copied().unwrap_or(default)
    }
    
    /// Map over the child
    #[inline]
    pub fn map<F, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(ElementId) -> T,
    {
        self.children.first().copied().map(f)
    }
    
    /// Map or return default
    #[inline]
    pub fn map_or<F, T>(&self, default: T, f: F) -> T
    where
        F: FnOnce(ElementId) -> T,
    {
        self.children.first().copied().map(f).unwrap_or(default)
    }
    
    /// Map or compute default
    #[inline]
    pub fn map_or_else<F, D, T>(&self, default: D, f: F) -> T
    where
        F: FnOnce(ElementId) -> T,
        D: FnOnce() -> T,
    {
        self.children.first().copied().map(f).unwrap_or_else(default)
    }
}

/// Fixed children (for Exact<N>)
#[derive(Debug, Clone, Copy)]
pub struct FixedChildren<'a, const N: usize> {
    children: &'a [ElementId; N],
}

impl<'a, const N: usize> ChildrenAccess for FixedChildren<'a, N> {
    fn as_slice(&self) -> &[ElementId] { self.children }
}

impl<'a> FixedChildren<'a, 1> {
    #[inline(always)]
    pub fn single(&self) -> ElementId { self.children[0] }
}

impl<'a> FixedChildren<'a, 2> {
    #[inline(always)]
    pub fn first(&self) -> ElementId { self.children[0] }
    #[inline(always)]
    pub fn second(&self) -> ElementId { self.children[1] }
    #[inline(always)]
    pub fn pair(&self) -> (ElementId, ElementId) { (self.children[0], self.children[1]) }
}

impl<'a> FixedChildren<'a, 3> {
    #[inline(always)]
    pub fn triple(&self) -> (ElementId, ElementId, ElementId) {
        (self.children[0], self.children[1], self.children[2])
    }
}

/// Slice children (for AtLeast<N> and Variable)
#[derive(Debug, Clone, Copy)]
pub struct SliceChildren<'a> {
    children: &'a [ElementId],
}

impl ChildrenAccess for SliceChildren<'_> {
    fn as_slice(&self) -> &[ElementId] { self.children }
}

impl<'a> SliceChildren<'a> {
    #[inline(always)]
    pub fn get(&self, index: usize) -> Option<ElementId> { self.children.get(index).copied() }
    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + '_ { self.children.iter().copied() }
    #[inline(always)]
    pub fn first(&self) -> Option<ElementId> { self.children.first().copied() }
    #[inline(always)]
    pub fn last(&self) -> Option<ElementId> { self.children.last().copied() }
}
```

---

## 5. Public Traits

### 5.1 HasTypedChildren Trait

```rust
/// Trait for contexts that provide typed children access
pub trait HasTypedChildren<A: Arity> {
    fn children(&self) -> A::Children<'_>;
}
```

### 5.2 Render Trait (Box Protocol)

```rust
use downcast_rs::{impl_downcast, Downcast};

/// Box protocol render object
///
/// Primary trait for standard 2D layout widgets.
pub trait Render<A: Arity>: Downcast + Send + Sync + Debug + 'static {
    /// Perform layout: BoxConstraints → Size
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, A>) -> Size;
    
    /// Paint to canvas
    fn paint(&self, ctx: &BoxPaintContext<'_, A>) -> Canvas;
    
    /// Hit testing
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, A>) -> bool;
    
    /// Debug name (default: type name)
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

impl_downcast!(Render<A> where A: Arity);
```

### 5.3 SliverRender Trait (Sliver Protocol)

```rust
/// Sliver protocol render object
///
/// For scrollable content with viewport awareness.
pub trait SliverRender<A: Arity>: Downcast + Send + Sync + Debug + 'static {
    /// Perform layout: SliverConstraints → SliverGeometry
    fn layout(&mut self, ctx: &mut SliverLayoutContext<'_, A>) -> SliverGeometry;
    
    /// Paint to canvas
    fn paint(&self, ctx: &SliverPaintContext<'_, A>) -> Canvas;
    
    /// Hit testing
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, A>) -> SliverHitTestResult;
    
    /// Debug name (default: type name)
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

impl_downcast!(SliverRender<A> where A: Arity);
```

---

## 6. Internal RenderObject Trait

```rust
/// Internal unified trait - auto-implemented via blanket impls
pub(crate) trait RenderObject<P: Protocol, A: Arity>: Downcast + Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &mut P::LayoutContext<'_, A>) -> P::Geometry;
    fn paint(&self, ctx: &P::PaintContext<'_, A>) -> Canvas;
    fn hit_test(&self, ctx: &mut P::HitTestContext<'_, A>) -> P::HitTestResult;
    fn debug_name(&self) -> &'static str;
}

impl_downcast!(RenderObject<P, A> where P: Protocol, A: Arity);

/// Blanket impl for Render<A>
impl<A, R> RenderObject<BoxProtocol, A> for R
where
    A: Arity,
    R: Render<A>,
{
    #[inline]
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, A>) -> Size {
        Render::layout(self, ctx)
    }
    
    #[inline]
    fn paint(&self, ctx: &BoxPaintContext<'_, A>) -> Canvas {
        Render::paint(self, ctx)
    }
    
    #[inline]
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, A>) -> bool {
        Render::hit_test(self, ctx)
    }
    
    #[inline]
    fn debug_name(&self) -> &'static str {
        Render::debug_name(self)
    }
}

/// Blanket impl for SliverRender<A>
impl<A, R> RenderObject<SliverProtocol, A> for R
where
    A: Arity,
    R: SliverRender<A>,
{
    #[inline]
    fn layout(&mut self, ctx: &mut SliverLayoutContext<'_, A>) -> SliverGeometry {
        SliverRender::layout(self, ctx)
    }
    
    #[inline]
    fn paint(&self, ctx: &SliverPaintContext<'_, A>) -> Canvas {
        SliverRender::paint(self, ctx)
    }
    
    #[inline]
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, A>) -> SliverHitTestResult {
        SliverRender::hit_test(self, ctx)
    }
    
    #[inline]
    fn debug_name(&self) -> &'static str {
        SliverRender::debug_name(self)
    }
}
```

---

## 7. Context Types

### 7.1 Box Contexts

```rust
/// Layout context for Box protocol
#[derive(Debug)]
pub struct BoxLayoutContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub constraints: BoxConstraints,
}

impl<'a, A: Arity> BoxLayoutContext<'a, A> {
    pub fn new(tree: &'a ElementTree, children: &'a [ElementId], constraints: BoxConstraints) -> Self {
        Self {
            tree,
            children: A::from_slice(children),
            constraints,
        }
    }
    
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
    
    pub fn layout_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout_box_child(child_id, constraints)
    }
}

impl<'a, A: Arity> HasTypedChildren<A> for BoxLayoutContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}

/// Paint context for Box protocol
#[derive(Debug)]
pub struct BoxPaintContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub offset: Offset,
}

impl<'a, A: Arity> BoxPaintContext<'a, A> {
    pub fn new(tree: &'a ElementTree, children: &'a [ElementId], offset: Offset) -> Self {
        Self {
            tree,
            children: A::from_slice(children),
            offset,
        }
    }
    
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
    
    pub fn paint_child(&self, child_id: ElementId, offset: Offset) -> Canvas {
        self.tree.paint_box_child(child_id, offset)
    }
}

impl<'a, A: Arity> HasTypedChildren<A> for BoxPaintContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}

/// Hit test context for Box protocol
#[derive(Debug)]
pub struct BoxHitTestContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub position: Offset,
    pub result: &'a mut HitTestResult,
}

impl<'a, A: Arity> BoxHitTestContext<'a, A> {
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
    
    pub fn hit_test_child(&mut self, child_id: ElementId, position: Offset) -> bool {
        self.tree.hit_test_box_child(child_id, position, self.result)
    }
}

impl<'a, A: Arity> HasTypedChildren<A> for BoxHitTestContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}
```

### 7.2 Sliver Contexts

```rust
/// Layout context for Sliver protocol
#[derive(Debug)]
pub struct SliverLayoutContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub constraints: SliverConstraints,
}

impl<'a, A: Arity> SliverLayoutContext<'a, A> {
    pub fn new(tree: &'a ElementTree, children: &'a [ElementId], constraints: SliverConstraints) -> Self {
        Self {
            tree,
            children: A::from_slice(children),
            constraints,
        }
    }
    
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
    
    pub fn layout_sliver_child(&self, child_id: ElementId, constraints: SliverConstraints) -> SliverGeometry {
        self.tree.layout_sliver_child(child_id, constraints)
    }
    
    pub fn layout_box_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout_box_child(child_id, constraints)
    }
}

impl<'a, A: Arity> HasTypedChildren<A> for SliverLayoutContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}

/// Paint context for Sliver protocol
#[derive(Debug)]
pub struct SliverPaintContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub offset: Offset,
}

impl<'a, A: Arity> SliverPaintContext<'a, A> {
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
}

impl<'a, A: Arity> HasTypedChildren<A> for SliverPaintContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}

/// Hit test context for Sliver protocol
#[derive(Debug)]
pub struct SliverHitTestContext<'a, A: Arity> {
    tree: &'a ElementTree,
    children: A::Children<'a>,
    pub position: Offset,
    pub main_axis_position: f32,
    pub cross_axis_position: f32,
}

impl<'a, A: Arity> SliverHitTestContext<'a, A> {
    #[inline(always)]
    pub fn children(&self) -> A::Children<'a> { self.children }
}

impl<'a, A: Arity> HasTypedChildren<A> for SliverHitTestContext<'a, A> {
    fn children(&self) -> A::Children<'a> { self.children }
}
```

---

## 8. RenderState

```rust
/// Unified render state
#[derive(Debug)]
pub enum RenderState {
    Box(BoxRenderState),
    Sliver(SliverRenderState),
}

impl RenderState {
    pub fn new_box() -> Self { Self::Box(BoxRenderState::new()) }
    pub fn new_sliver() -> Self { Self::Sliver(SliverRenderState::new()) }
    
    pub fn for_protocol(protocol: LayoutProtocol) -> Self {
        match protocol {
            LayoutProtocol::Box => Self::new_box(),
            LayoutProtocol::Sliver => Self::new_sliver(),
        }
    }
    
    #[inline(always)]
    pub fn protocol(&self) -> LayoutProtocol {
        match self {
            Self::Box(_) => LayoutProtocol::Box,
            Self::Sliver(_) => LayoutProtocol::Sliver,
        }
    }
    
    #[inline(always)]
    pub fn offset(&self) -> Offset {
        match self {
            Self::Box(s) => s.offset,
            Self::Sliver(s) => s.offset,
        }
    }
    
    pub fn set_offset(&mut self, offset: Offset) {
        match self {
            Self::Box(s) => s.offset = offset,
            Self::Sliver(s) => s.offset = offset,
        }
    }
    
    #[inline(always)]
    pub fn flags(&self) -> &AtomicRenderFlags {
        match self {
            Self::Box(s) => &s.flags,
            Self::Sliver(s) => &s.flags,
        }
    }
    
    pub fn needs_layout(&self) -> bool { self.flags().needs_layout() }
    pub fn needs_paint(&self) -> bool { self.flags().needs_paint() }
    pub fn mark_needs_layout(&self) { self.flags().mark_needs_layout(); }
    pub fn mark_needs_paint(&self) { self.flags().mark_needs_paint(); }
    
    pub fn as_box(&self) -> Option<&BoxRenderState> {
        match self { Self::Box(s) => Some(s), _ => None }
    }
    
    pub fn as_box_mut(&mut self) -> Option<&mut BoxRenderState> {
        match self { Self::Box(s) => Some(s), _ => None }
    }
    
    pub fn as_sliver(&self) -> Option<&SliverRenderState> {
        match self { Self::Sliver(s) => Some(s), _ => None }
    }
    
    pub fn as_sliver_mut(&mut self) -> Option<&mut SliverRenderState> {
        match self { Self::Sliver(s) => Some(s), _ => None }
    }
    
    pub fn size(&self) -> Option<Size> { self.as_box().map(|s| s.size) }
    pub fn geometry(&self) -> Option<&SliverGeometry> { self.as_sliver().map(|s| &s.geometry) }
}

#[derive(Debug)]
pub struct BoxRenderState {
    pub size: Size,
    pub constraints: BoxConstraints,
    pub offset: Offset,
    pub flags: AtomicRenderFlags,
}

impl BoxRenderState {
    pub fn new() -> Self {
        Self {
            size: Size::ZERO,
            constraints: BoxConstraints::default(),
            offset: Offset::ZERO,
            flags: AtomicRenderFlags::new(),
        }
    }
}

#[derive(Debug)]
pub struct SliverRenderState {
    pub geometry: SliverGeometry,
    pub constraints: SliverConstraints,
    pub offset: Offset,
    pub flags: AtomicRenderFlags,
}

impl SliverRenderState {
    pub fn new() -> Self {
        Self {
            geometry: SliverGeometry::ZERO,
            constraints: SliverConstraints::default(),
            offset: Offset::ZERO,
            flags: AtomicRenderFlags::new(),
        }
    }
}
```

---

## 9. Type Erasure Layer

### 9.1 DynRenderObject Trait

```rust
use downcast_rs::{impl_downcast, Downcast};

/// Type-erased render object
///
/// NOTE: protocol() and runtime_arity() are NOT included here.
/// The source of truth is RenderElement fields.
pub trait DynRenderObject: Downcast + Send + Sync + Debug + 'static {
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry;
    
    fn dyn_paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> Canvas;
    
    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> DynHitTestResult;
    
    fn debug_name(&self) -> &'static str;
}

impl_downcast!(DynRenderObject);

#[derive(Debug, Clone)]
pub enum DynConstraints {
    Box(BoxConstraints),
    Sliver(SliverConstraints),
}

impl DynConstraints {
    pub fn as_box(&self) -> &BoxConstraints {
        match self { Self::Box(c) => c, _ => panic!("Expected Box constraints") }
    }
    
    pub fn as_sliver(&self) -> &SliverConstraints {
        match self { Self::Sliver(c) => c, _ => panic!("Expected Sliver constraints") }
    }
}

#[derive(Debug, Clone)]
pub enum DynGeometry {
    Box(Size),
    Sliver(SliverGeometry),
}

#[derive(Debug, Clone)]
pub enum DynHitTestResult {
    Box(bool),
    Sliver(SliverHitTestResult),
}
```

### 9.2 Safe Wrappers

```rust
/// Box protocol wrapper
pub struct BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    inner: R,
    _phantom: PhantomData<A>,
}

impl<A, R> BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    pub fn new(render_object: R) -> Self {
        Self { inner: render_object, _phantom: PhantomData }
    }
}

impl<A, R> Debug for BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxRenderObjectWrapper")
            .field("inner", &self.inner)
            .field("arity", &A::runtime_arity())
            .finish()
    }
}

impl<A, R> DynRenderObject for BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        // Strict validation in release mode when feature enabled
        #[cfg(feature = "strict-arity")]
        if !A::validate_count(children.len()) {
            panic!(
                "Arity violation in {}: expected {}, got {} children",
                self.inner.debug_name(), A::runtime_arity(), children.len()
            );
        }
        
        // Debug-only validation in hot-path (zero cost in release)
        #[cfg(not(feature = "strict-arity"))]
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {}, got {} children",
            self.inner.debug_name(), A::runtime_arity(), children.len()
        );
        
        let mut ctx = BoxLayoutContext::<A>::new(tree, children, *constraints.as_box());
        DynGeometry::Box(self.inner.layout(&mut ctx))
    }
    
    fn dyn_paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> Canvas {
        let ctx = BoxPaintContext::<A>::new(tree, children, offset);
        self.inner.paint(&ctx)
    }
    
    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> DynHitTestResult {
        let mut ctx = BoxHitTestContext {
            tree,
            children: A::from_slice(children),
            position,
            result,
        };
        DynHitTestResult::Box(self.inner.hit_test(&mut ctx))
    }
    
    fn debug_name(&self) -> &'static str { self.inner.debug_name() }
}

/// Sliver protocol wrapper
pub struct SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    inner: R,
    _phantom: PhantomData<A>,
}

impl<A, R> SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    pub fn new(render_object: R) -> Self {
        Self { inner: render_object, _phantom: PhantomData }
    }
}

impl<A, R> Debug for SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverRenderObjectWrapper")
            .field("inner", &self.inner)
            .field("arity", &A::runtime_arity())
            .finish()
    }
}

impl<A, R> DynRenderObject for SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        // Strict validation in release mode when feature enabled
        #[cfg(feature = "strict-arity")]
        if !A::validate_count(children.len()) {
            panic!(
                "Arity violation in {}: expected {}, got {} children",
                self.inner.debug_name(), A::runtime_arity(), children.len()
            );
        }
        
        // Debug-only validation in hot-path (zero cost in release)
        #[cfg(not(feature = "strict-arity"))]
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {}, got {} children",
            self.inner.debug_name(), A::runtime_arity(), children.len()
        );
        
        let mut ctx = SliverLayoutContext::<A>::new(tree, children, constraints.as_sliver().clone());
        DynGeometry::Sliver(self.inner.layout(&mut ctx))
    }
    
    fn dyn_paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> Canvas {
        let ctx = SliverPaintContext {
            tree,
            children: A::from_slice(children),
            offset,
        };
        self.inner.paint(&ctx)
    }
    
    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> DynHitTestResult {
        let mut ctx = SliverHitTestContext {
            tree,
            children: A::from_slice(children),
            position,
            main_axis_position: 0.0,
            cross_axis_position: 0.0,
        };
        DynHitTestResult::Sliver(self.inner.hit_test(&mut ctx))
    }
    
    fn debug_name(&self) -> &'static str { self.inner.debug_name() }
}
```

---

## 10. RenderElement

```rust
/// Unified element for all render objects
pub struct RenderElement {
    base: ElementBase,
    render_object: RwLock<Box<dyn DynRenderObject>>,
    render_state: RwLock<RenderState>,
    parent_data: Option<Box<dyn ParentData>>,
    children: Vec<ElementId>,
    pending_children: Vec<ElementId>,
    
    /// Source of truth for protocol (NOT duplicated in DynRenderObject)
    protocol: LayoutProtocol,
    
    /// Source of truth for arity (NOT duplicated in DynRenderObject)
    arity: RuntimeArity,
    
    /// Flag for transactional children updates
    updating_children: bool,
}

impl RenderElement {
    // ========== Box Protocol Constructors ==========
    
    pub fn box_leaf<R: Render<Leaf>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(BoxRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_box()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Box,
            arity: RuntimeArity::Exact(0),
            updating_children: false,
        }
    }
    
    pub fn box_optional<R: Render<Optional>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(BoxRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_box()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Box,
            arity: RuntimeArity::Optional,
            updating_children: false,
        }
    }
    
    pub fn box_single<R: Render<Single>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(BoxRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_box()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Box,
            arity: RuntimeArity::Exact(1),
            updating_children: false,
        }
    }
    
    pub fn box_pair<R: Render<Pair>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(BoxRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_box()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Box,
            arity: RuntimeArity::Exact(2),
            updating_children: false,
        }
    }
    
    pub fn box_variable<R: Render<Variable>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(BoxRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_box()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Box,
            arity: RuntimeArity::Variable,
            updating_children: false,
        }
    }
    
    // ========== Sliver Protocol Constructors ==========
    
    pub fn sliver_single<R: SliverRender<Single>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(SliverRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_sliver()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Sliver,
            arity: RuntimeArity::Exact(1),
            updating_children: false,
        }
    }
    
    pub fn sliver_variable<R: SliverRender<Variable>>(render: R) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(Box::new(SliverRenderObjectWrapper::new(render))),
            render_state: RwLock::new(RenderState::new_sliver()),
            parent_data: None,
            children: Vec::new(),
            pending_children: Vec::new(),
            protocol: LayoutProtocol::Sliver,
            arity: RuntimeArity::Variable,
            updating_children: false,
        }
    }
    
    // ========== Transactional Children Update API ==========
    
    /// Begin batch children update (disables intermediate validation)
    ///
    /// Use this when you need to remove and add children atomically.
    /// Call `commit_children_update()` when done.
    pub fn begin_children_update(&mut self) {
        self.updating_children = true;
    }
    
    /// Commit batch children update (validates final state)
    ///
    /// Panics if final children count violates arity.
    pub fn commit_children_update(&mut self) {
        self.updating_children = false;
        if !self.arity.validate(self.children.len()) {
            panic!(
                "Arity violation after children update: expected {}, got {} children",
                self.arity, self.children.len()
            );
        }
        self.mark_needs_layout();
    }
    
    // ========== Arity-Safe Child Management ==========
    
    /// Add a child (validates arity unless in transactional mode)
    pub fn push_child(&mut self, child_id: ElementId) {
        if !self.updating_children {
            let new_count = self.children.len() + 1;
            let name = self.render_object.read().debug_name();
            match self.arity {
                RuntimeArity::Exact(n) if new_count > n => {
                    panic!(
                        "[{}] Arity violation: cannot add child {:?}, already has max {} children (current: {})",
                        name, child_id, n, self.children.len()
                    );
                }
                RuntimeArity::Optional if new_count > 1 => {
                    panic!(
                        "[{}] Arity violation: cannot add child {:?}, Optional already has 1",
                        name, child_id
                    );
                }
                _ => {}
            }
        }
        self.children.push(child_id);
        if !self.updating_children {
            self.mark_needs_layout();
        }
    }
    
    /// Atomically replace all children (recommended for rebuilds)
    ///
    /// This is the safest way to update children during tree reconstruction.
    pub fn replace_children(&mut self, children: Vec<ElementId>) {
        if !self.arity.validate(children.len()) {
            panic!(
                "Arity violation: cannot set {} children, expected {}",
                children.len(), self.arity
            );
        }
        self.children = children;
        self.mark_needs_layout();
    }
    
    /// Remove a child by ID (pub(crate) - use replace_children for external code)
    ///
    /// # Warning
    /// 
    /// Can temporarily violate arity invariant. This is `pub(crate)` for a reason!
    /// 
    /// External code should use:
    /// - `replace_children()` for atomic updates (recommended)
    /// - `begin/commit_children_update()` for complex transactions
    ///
    /// # Safety Invariant
    ///
    /// During a transaction (`updating_children = true`), this can leave
    /// the element in an invalid state. Always call `commit_children_update()`
    /// or the element will panic on next non-transactional operation.
    ///
    /// # Example
    /// 
    /// ```rust,ignore
    /// // ❌ DON'T do this for Exact/AtLeast arities:
    /// element.remove_child(old);  // May panic!
    /// element.push_child(new);
    /// 
    /// // ✅ DO this instead:
    /// element.replace_children(new_children);
    /// 
    /// // ✅ OR use transactions:
    /// element.begin_children_update();
    /// element.remove_child(old);  // Safe during transaction
    /// element.push_child(new);
    /// element.commit_children_update();
    /// ```
    pub(crate) fn remove_child(&mut self, child_id: ElementId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child_id) {
            if !self.updating_children {
                let new_count = self.children.len() - 1;
                let name = self.render_object.read().debug_name();
                match self.arity {
                    RuntimeArity::Exact(n) if new_count < n => {
                        panic!(
                            "[{}] Arity violation: cannot remove child {:?}, needs exactly {} children (would have: {})",
                            name, child_id, n, new_count
                        );
                    }
                    RuntimeArity::AtLeast(n) if new_count < n => {
                        panic!(
                            "[{}] Arity violation: cannot remove child {:?}, needs >= {} children (would have: {})",
                            name, child_id, n, new_count
                        );
                    }
                    _ => {}
                }
            }
            self.children.remove(pos);
            if !self.updating_children {
                self.mark_needs_layout();
            }
            true
        } else {
            false
        }
    }
    
    pub fn children(&self) -> &[ElementId] { &self.children }
    
    // ========== Accessors (Single Source of Truth) ==========
    
    #[inline(always)]
    pub fn protocol(&self) -> LayoutProtocol { self.protocol }
    
    #[inline(always)]
    pub fn runtime_arity(&self) -> RuntimeArity { self.arity }
    
    #[inline(always)]
    pub fn is_box(&self) -> bool { self.protocol == LayoutProtocol::Box }
    
    #[inline(always)]
    pub fn is_sliver(&self) -> bool { self.protocol == LayoutProtocol::Sliver }
    
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, Box<dyn DynRenderObject>> {
        self.render_object.read()
    }
    
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Box<dyn DynRenderObject>> {
        self.render_object.write()
    }
    
    pub fn render_state(&self) -> &RwLock<RenderState> { &self.render_state }
    
    /// Mark needs layout (sets flag only - use ElementTree::request_layout for full scheduling)
    pub fn mark_needs_layout(&self) { self.render_state.read().mark_needs_layout(); }
    
    /// Mark needs paint (sets flag only - use ElementTree::request_paint for full scheduling)
    pub fn mark_needs_paint(&self) { self.render_state.read().mark_needs_paint(); }
    
    // ========== Layout/Paint/HitTest ==========
    
    pub fn layout(&self, tree: &ElementTree, constraints: DynConstraints) -> DynGeometry {
        let mut ro = self.render_object.write();
        let geometry = ro.dyn_layout(tree, &self.children, &constraints);
        
        {
            let mut state = self.render_state.write();
            match (&mut *state, &geometry) {
                (RenderState::Box(s), DynGeometry::Box(size)) => {
                    s.size = *size;
                    s.constraints = *constraints.as_box();
                }
                (RenderState::Sliver(s), DynGeometry::Sliver(geom)) => {
                    s.geometry = geom.clone();
                    s.constraints = constraints.as_sliver().clone();
                }
                _ => panic!(
                    "Protocol mismatch: state={:?}, geometry={:?}",
                    state.protocol(), 
                    match geometry { DynGeometry::Box(_) => "Box", DynGeometry::Sliver(_) => "Sliver" }
                ),
            }
            state.flags().clear_needs_layout();
        }
        
        geometry
    }
    
    pub fn paint(&self, tree: &ElementTree, offset: Offset) -> Canvas {
        let ro = self.render_object.read();
        let canvas = ro.dyn_paint(tree, &self.children, offset);
        self.render_state.read().flags().clear_needs_paint();
        canvas
    }
    
    pub fn hit_test(
        &self,
        tree: &ElementTree,
        position: Offset,
        result: &mut HitTestResult,
    ) -> DynHitTestResult {
        let ro = self.render_object.read();
        ro.dyn_hit_test(tree, &self.children, position, result)
    }
}

impl Debug for RenderElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NOTE: This acquires a read lock - avoid in hot paths
        f.debug_struct("RenderElement")
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("children", &self.children.len())
            .field("name", &self.render_object.read().debug_name())
            .finish()
    }
}
```

---

## 11. Element Enum

```rust
/// Element - 3 variants
#[derive(Debug)]
pub enum Element {
    Component(ComponentElement),
    Render(RenderElement),
    Provider(ProviderElement),
}

impl Element {
    pub fn as_component(&self) -> Option<&ComponentElement> {
        match self { Self::Component(c) => Some(c), _ => None }
    }
    
    pub fn as_component_mut(&mut self) -> Option<&mut ComponentElement> {
        match self { Self::Component(c) => Some(c), _ => None }
    }
    
    pub fn as_render(&self) -> Option<&RenderElement> {
        match self { Self::Render(r) => Some(r), _ => None }
    }
    
    pub fn as_render_mut(&mut self) -> Option<&mut RenderElement> {
        match self { Self::Render(r) => Some(r), _ => None }
    }
    
    pub fn as_provider(&self) -> Option<&ProviderElement> {
        match self { Self::Provider(p) => Some(p), _ => None }
    }
    
    pub fn as_provider_mut(&mut self) -> Option<&mut ProviderElement> {
        match self { Self::Provider(p) => Some(p), _ => None }
    }
    
    pub fn is_component(&self) -> bool { matches!(self, Self::Component(_)) }
    pub fn is_render(&self) -> bool { matches!(self, Self::Render(_)) }
    pub fn is_provider(&self) -> bool { matches!(self, Self::Provider(_)) }
    
    pub fn parent(&self) -> Option<ElementId> {
        match self {
            Self::Component(c) => c.parent(),
            Self::Render(r) => r.base.parent(),
            Self::Provider(p) => p.parent(),
        }
    }
    
    pub fn lifecycle(&self) -> ElementLifecycle {
        match self {
            Self::Component(c) => c.lifecycle(),
            Self::Render(r) => r.base.lifecycle(),
            Self::Provider(p) => p.lifecycle(),
        }
    }
}
```

---

## 12. ElementTree Integration

```rust
impl ElementTree {
    // ========== Centralized Scheduling ==========
    
    /// Request layout for an element
    ///
    /// This is the correct way to schedule layout. It:
    /// 1. Adds element to dirty layout set
    /// 2. Sets needs_layout flag in RenderState
    ///
    /// Always use this instead of calling mark_needs_layout() directly.
    pub fn request_layout(&mut self, element_id: ElementId) {
        // Add to dirty set for coordinator
        self.dirty_layout.insert(element_id);
        
        // Set flag in render state
        if let Some(Element::Render(render)) = self.get(element_id) {
            render.render_state.read().mark_needs_layout();
        }
        
        tracing::trace!(element_id = ?element_id, "request_layout");
    }
    
    /// Request paint for an element
    ///
    /// This is the correct way to schedule paint. It:
    /// 1. Adds element to dirty paint set
    /// 2. Sets needs_paint flag in RenderState
    ///
    /// Always use this instead of calling mark_needs_paint() directly.
    pub fn request_paint(&mut self, element_id: ElementId) {
        // Add to dirty set for coordinator
        self.dirty_paint.insert(element_id);
        
        // Set flag in render state
        if let Some(Element::Render(render)) = self.get(element_id) {
            render.render_state.read().mark_needs_paint();
        }
        
        tracing::trace!(element_id = ?element_id, "request_paint");
    }
    
    // ========== Layout Helpers ==========
    
    pub fn layout_box_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // ... existing implementation
    }
    
    pub fn layout_sliver_child(&self, child_id: ElementId, constraints: SliverConstraints) -> SliverGeometry {
        // ... existing implementation
    }
    
    // ========== Paint Helpers ==========
    
    pub fn paint_box_child(&self, child_id: ElementId, offset: Offset) -> Canvas {
        // ... existing implementation
    }
    
    // ========== Hit Test Helpers ==========
    
    pub fn hit_test_box_child(&self, child_id: ElementId, position: Offset, result: &mut HitTestResult) -> bool {
        // ... existing implementation
    }
}
```

---

## 13. Thread Safety Guarantees

### 13.1 Invariants

- **Layout and paint NEVER execute concurrently for the same element**
- Multiple elements CAN be laid out in parallel (with `parallel` feature)
- Read locks for flag checks, write locks for state updates

### 13.2 Lock Order

To prevent deadlocks, always acquire locks in this order:

1. `render_object` (if needed)
2. `render_state`

**Never acquire in reverse order.**

### 13.3 Lock Usage Patterns

| Operation | Lock Type | Reason |
|-----------|-----------|--------|
| `mark_needs_layout()` | Read | Atomic flags inside |
| `mark_needs_paint()` | Read | Atomic flags inside |
| `layout()` | Write | Updates size/constraints |
| `paint()` | Read (object) + Read (state) | Only reads computed values |
| `Debug::fmt()` | Read | Reads debug_name |

### 13.4 Performance Notes

- `RenderElement::Debug` acquires read lock - avoid in hot traces
- Flag checks via `needs_layout()`/`needs_paint()` are lock-free (atomic)
- parking_lot::RwLock is 2-3x faster than std::sync::RwLock

---

## 14. Usage Examples

### 14.1 Leaf - Text (no children)

```rust
#[derive(Debug)]
pub struct RenderText {
    text: String,
    style: TextStyle,
}

impl Render<Leaf> for RenderText {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf>) -> Size {
        let measured = measure_text(&self.text, &self.style, ctx.constraints.max_width);
        Size::new(measured.width, measured.height)
    }
    
    fn paint(&self, ctx: &BoxPaintContext<'_, Leaf>) -> Canvas {
        let mut canvas = Canvas::new();
        canvas.draw_text(&self.text, ctx.offset, &self.style);
        canvas
    }
    
    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf>) -> bool {
        true
    }
}
```

### 14.2 Optional - SizedBox (0 or 1 child)

```rust
#[derive(Debug)]
pub struct RenderSizedBox {
    width: Option<f32>,
    height: Option<f32>,
}

impl Render<Optional> for RenderSizedBox {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Optional>) -> Size {
        let child_constraints = BoxConstraints {
            min_width: self.width.unwrap_or(0.0),
            max_width: self.width.unwrap_or(ctx.constraints.max_width),
            min_height: self.height.unwrap_or(0.0),
            max_height: self.height.unwrap_or(ctx.constraints.max_height),
        };
        
        // Layout child if present
        let child_size = ctx.children().map_or(Size::ZERO, |child| {
            ctx.layout_child(child, child_constraints)
        });
        
        Size::new(
            self.width.unwrap_or(child_size.width),
            self.height.unwrap_or(child_size.height),
        )
    }
    
    fn paint(&self, ctx: &BoxPaintContext<'_, Optional>) -> Canvas {
        ctx.children()
            .map(|child| ctx.paint_child(child, ctx.offset))
            .unwrap_or_else(Canvas::new)
    }
    
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Optional>) -> bool {
        ctx.children()
            .map(|child| ctx.hit_test_child(child, ctx.position))
            .unwrap_or(true)
    }
}
```

### 14.3 Single - Padding (exactly 1 child)

```rust
#[derive(Debug)]
pub struct RenderPadding {
    padding: EdgeInsets,
}

impl Render<Single> for RenderPadding {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single>) -> Size {
        let child = ctx.children().single();
        let child_constraints = ctx.constraints.deflate(self.padding);
        let child_size = ctx.layout_child(child, child_constraints);
        
        Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        )
    }
    
    fn paint(&self, ctx: &BoxPaintContext<'_, Single>) -> Canvas {
        let child = ctx.children().single();
        ctx.paint_child(child, ctx.offset + self.padding.top_left())
    }
    
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single>) -> bool {
        let child = ctx.children().single();
        ctx.hit_test_child(child, ctx.position - self.padding.top_left())
    }
}
```

### 14.4 Variable - Flex (any children)

```rust
#[derive(Debug)]
pub struct RenderFlex {
    direction: Axis,
}

impl Render<Variable> for RenderFlex {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable>) -> Size {
        let mut main_used = 0.0;
        let mut cross_max = 0.0;
        
        for child in ctx.children().iter() {
            let child_size = ctx.layout_child(child, ctx.constraints);
            main_used += child_size.main(self.direction);
            cross_max = cross_max.max(child_size.cross(self.direction));
        }
        
        Size::from_main_cross(self.direction, main_used, cross_max)
    }
    
    fn paint(&self, ctx: &BoxPaintContext<'_, Variable>) -> Canvas {
        let mut canvas = Canvas::new();
        for child in ctx.children().iter() {
            canvas.merge(ctx.paint_child(child, ctx.offset));
        }
        canvas
    }
    
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable>) -> bool {
        for child in ctx.children().as_slice().iter().rev() {
            if ctx.hit_test_child(*child, ctx.position) {
                return true;
            }
        }
        false
    }
}
```

### 14.5 Transactional Children Update

```rust
// ========== When to Use Each Approach ==========

// ✅ Use replace_children when:
// - Complete replacement of all children (reconciliation)
// - Single atomic operation
// - Most common case
element.replace_children(new_children);

// ✅ Use transactions when:
// - Complex logic with conditions
// - Multiple operations that depend on each other
// - Need to inspect intermediate state
// - Performance-critical scenarios avoiding Vec allocation
element.begin_children_update();

for old_child in old_children {
    if !should_keep(old_child) {
        element.remove_child(old_child);
    }
}

for new_child in new_children {
    if !element.children().contains(&new_child) {
        element.push_child(new_child);
    }
}

element.commit_children_update();  // Validates final state

// ❌ DON'T use direct operations for Exact/AtLeast arities:
// This will panic if arity is violated at any step!
element.remove_child(old);  // May panic!
element.push_child(new);    // May not even reach here

// ========== Full Example: Reconciliation ==========

fn rebuild_children(element: &mut RenderElement, new_children: Vec<ElementId>) {
    // Option 1: Atomic replace (recommended for most cases)
    element.replace_children(new_children);
    
    // Option 2: Transactional update (for complex logic)
    element.begin_children_update();
    
    // Safe to temporarily violate arity during transaction
    while let Some(&child) = element.children().first() {
        element.remove_child(child);
    }
    
    for child in new_children {
        element.push_child(child);
    }
    
    // Validates that final state matches arity
    element.commit_children_update();
}

// ========== Edge Case: Swapping Two Children ==========

fn swap_children(element: &mut RenderElement, a: ElementId, b: ElementId) {
    element.begin_children_update();
    
    let children = element.children().to_vec();
    let pos_a = children.iter().position(|&id| id == a);
    let pos_b = children.iter().position(|&id| id == b);
    
    if let (Some(i), Some(j)) = (pos_a, pos_b) {
        let mut new_children = children;
        new_children.swap(i, j);
        
        // Clear and rebuild - safe during transaction
        while element.children().len() > 0 {
            element.remove_child(element.children()[0]);
        }
        for child in new_children {
            element.push_child(child);
        }
    }
    
    element.commit_children_update();
}
```

---

## 15. Migration Guide

### From Old Render Trait

**Before:**
```rust
impl Render for RenderPadding {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let child = ctx.children.single().expect("needs child");
        // ...
    }
    
    fn arity(&self) -> Arity { Arity::Exact(1) }
    fn as_any(&self) -> &dyn Any { self }
}
```

**After:**
```rust
impl Render<Single> for RenderPadding {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single>) -> Size {
        let child = ctx.children().single();  // No unwrap! Compile-time guarantee
        // ...
    }
    
    // No more arity() or as_any() boilerplate!
}
```

---

## 16. Implementation Plan

| Week | Phase | Tasks |
|------|-------|-------|
| 1 | Foundation | Arity system (including Optional), Protocol system, HasTypedChildren |
| 2 | Traits | `Render<A>`, `SliverRender<A>`, blanket impls |
| 3 | Wrappers | `DynRenderObject` (without protocol/arity), safe wrappers |
| 4 | Element | `RenderElement` with transactional API, ElementTree integration |
| 5 | Migrate | All render objects |
| 6 | Pipeline | Centralized scheduling, benchmarks |

---

## 17. Testing Strategy

### 17.1 Test Categories

- **Property-based**: Quickcheck for arity validation (including Optional, transactional)
- **Miri**: Memory safety verification
- **Criterion**: Performance benchmarks vs legacy
- **Loom**: Concurrency tests for lock ordering
- **Debug assertions**: Verify hot-path validation only runs in debug

### 17.2 Loom Deadlock Prevention Test

```rust
#[cfg(loom)]
#[test]
fn test_lock_order_no_deadlock() {
    use loom::sync::Arc;
    use loom::thread;
    
    loom::model(|| {
        let element = Arc::new(RenderElement::box_single(TestRender));
        let e1 = Arc::clone(&element);
        let e2 = Arc::clone(&element);
        
        // Thread 1: correct lock order
        let t1 = thread::spawn(move || {
            let _obj = e1.render_object();    // Lock 1
            let _state = e1.render_state();   // Lock 2
        });
        
        // Thread 2: also correct lock order
        let t2 = thread::spawn(move || {
            let _obj = e2.render_object();    // Lock 1
            let _state = e2.render_state();   // Lock 2
        });
        
        t1.join().unwrap();
        t2.join().unwrap();
    });
}

#[cfg(loom)]
#[test]
#[should_panic]  // loom detects deadlock
fn test_lock_order_detects_deadlock() {
    use loom::sync::Arc;
    use loom::thread;
    
    loom::model(|| {
        let element = Arc::new(RenderElement::box_single(TestRender));
        let e1 = Arc::clone(&element);
        let e2 = Arc::clone(&element);
        
        // Thread 1: correct order
        let t1 = thread::spawn(move || {
            let _obj = e1.render_object();
            let _state = e1.render_state();
        });
        
        // Thread 2: WRONG order - will deadlock!
        let t2 = thread::spawn(move || {
            let _state = e2.render_state();   // Lock 2 first
            let _obj = e2.render_object();    // Lock 1 second - DEADLOCK
        });
        
        t1.join().unwrap();
        t2.join().unwrap();
    });
}
```

### 17.3 Benchmark: Verify debug_assert Zero Cost

```rust
use criterion::{criterion_group, criterion_main, Criterion, black_box};

fn bench_layout_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout_validation");
    
    let tree = setup_tree();
    let element = setup_element();
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    
    // This should be identical in release mode regardless of validation
    group.bench_function("layout_release", |b| {
        b.iter(|| {
            black_box(element.layout(&tree, DynConstraints::Box(constraints)))
        });
    });
    
    group.finish();
}

fn bench_from_slice(c: &mut Criterion) {
    let mut group = c.benchmark_group("from_slice");
    
    let children: Vec<ElementId> = (0..10).map(ElementId::new).collect();
    
    group.bench_function("variable", |b| {
        b.iter(|| {
            black_box(Variable::from_slice(&children))
        });
    });
    
    group.bench_function("single", |b| {
        let single_child = &children[0..1];
        b.iter(|| {
            black_box(Single::from_slice(single_child))
        });
    });
    
    group.finish();
}

criterion_group!(benches, bench_layout_validation, bench_from_slice);
criterion_main!(benches);
```

### 17.4 Arity Validation Tests

```rust
#[test]
fn test_transactional_update() {
    let mut element = RenderElement::box_single(TestRender);
    let child1 = ElementId::new(1);
    let child2 = ElementId::new(2);
    
    element.push_child(child1);
    
    // Transactional swap
    element.begin_children_update();
    element.remove_child(child1);  // Now 0 children - temporarily invalid!
    element.push_child(child2);    // Now 1 child - valid again
    element.commit_children_update();
    
    assert_eq!(element.children(), &[child2]);
}

#[test]
#[should_panic(expected = "Arity violation")]
fn test_invalid_commit_panics() {
    let mut element = RenderElement::box_single(TestRender);
    
    element.begin_children_update();
    // Don't add any children
    element.commit_children_update();  // Panics: Single needs exactly 1
}

#[test]
fn test_replace_children_atomic() {
    let mut element = RenderElement::box_variable(TestRender);
    let children: Vec<_> = (0..5).map(ElementId::new).collect();
    
    element.replace_children(children.clone());
    assert_eq!(element.children().len(), 5);
}
```

---

## 18. Performance Considerations

- Cache `protocol` and `arity` on `RenderElement` (single source of truth)
- Use `#[inline(always)]` for hot-path accessors
- `downcast-rs` has minimal overhead
- `RwLock` contention manageable for typical UI trees
- `debug_assert!` in hot-path validation (zero cost in release)
- Avoid `Debug::fmt` in hot traces (acquires lock)

---

## 19. API Reference

### Public Traits

```rust
pub trait Render<A: Arity>: Downcast + Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, A>) -> Size;
    fn paint(&self, ctx: &BoxPaintContext<'_, A>) -> Canvas;
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, A>) -> bool;
    fn debug_name(&self) -> &'static str { ... }
}

pub trait SliverRender<A: Arity>: Downcast + Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &mut SliverLayoutContext<'_, A>) -> SliverGeometry;
    fn paint(&self, ctx: &SliverPaintContext<'_, A>) -> Canvas;
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, A>) -> SliverHitTestResult;
    fn debug_name(&self) -> &'static str { ... }
}

pub trait HasTypedChildren<A: Arity> {
    fn children(&self) -> A::Children<'_>;
}
```

### Arity Types

```rust
pub struct Leaf;              // 0 children
pub struct Optional;          // 0 or 1 child
pub type Single = Exact<1>;   // 1 child
pub type Pair = Exact<2>;     // 2 children
pub type Triple = Exact<3>;   // 3 children
pub struct Variable;          // 0..∞ children
pub struct AtLeast<const N: usize>; // N..∞ children
```

### Children Accessors

| Arity | Accessor | Key Methods |
|-------|----------|-------------|
| `Leaf` | `NoChildren` | - |
| `Optional` | `OptionalChild` | `get()`, `map()`, `is_some()`, `is_none()` |
| `Single` | `FixedChildren<1>` | `single()` |
| `Pair` | `FixedChildren<2>` | `pair()`, `first()`, `second()` |
| `Variable` | `SliceChildren` | `iter()`, `get(i)`, `first()`, `last()` |

### RenderElement Constructors

```rust
RenderElement::box_leaf(render)      // Render<Leaf>
RenderElement::box_optional(render)  // Render<Optional>
RenderElement::box_single(render)    // Render<Single>
RenderElement::box_pair(render)      // Render<Pair>
RenderElement::box_variable(render)  // Render<Variable>
RenderElement::sliver_single(render) // SliverRender<Single>
RenderElement::sliver_variable(render) // SliverRender<Variable>
```

### Transactional API

```rust
element.begin_children_update();  // Disable intermediate validation
element.remove_child(id);         // Safe during transaction
element.push_child(id);           // Safe during transaction
element.commit_children_update(); // Validate final state

element.replace_children(vec);    // Atomic replacement (recommended)
```

### Scheduling API

```rust
tree.request_layout(element_id);  // Adds to dirty set + sets flag
tree.request_paint(element_id);   // Adds to dirty set + sets flag
```

---

## Changelog

- **v1.0** - Initial specification
- **v1.1** - Removed unsafe, added arity enforcement, macros
- **v1.2** - Public traits, downcast-rs, dyn-clone, removed macros
- **v1.3** - Added `Optional` arity type, `OptionalChild` accessor
- **v1.4** - Fixed source of truth duplication, debug_assert in hot-path, transactional API, centralized scheduling, thread safety docs
- **v1.5** - Feature flag `strict-arity`, `#[inline(always)]` on from_slice, improved error messages with context, loom/benchmark examples, enhanced safety docs

---

*Document prepared for FLUI Framework*
*Last updated: November 2025*