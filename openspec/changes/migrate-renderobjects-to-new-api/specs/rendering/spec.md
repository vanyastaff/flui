# Rendering Capability Delta

## MODIFIED Requirements

### Requirement: RenderObject Arity System
All RenderObjects SHALL implement the appropriate arity-specific trait based on their child count characteristics.

#### Scenario: Leaf RenderObject with zero children
- **GIVEN** a RenderObject that never has children (e.g., RenderEditableLine, RenderImage)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBox<Leaf>`
- **AND** attempting to access children SHALL result in a compile error

#### Scenario: Optional RenderObject with 0-1 children
- **GIVEN** a RenderObject that functions meaningfully without a child (e.g., RenderSizedBox as spacer)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBox<Optional>`
- **AND** `ctx.children.get()` SHALL return `Option<NonZeroUsize>`
- **AND** layout logic SHALL handle `None` case gracefully

#### Scenario: Single RenderObject with exactly 1 child
- **GIVEN** a RenderObject that requires exactly one child (e.g., RenderPadding, RenderOpacity)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBox<Single>` or `SliverRender<Single>`
- **AND** `ctx.children.single()` SHALL return `NonZeroUsize` directly
- **AND** child existence SHALL be guaranteed at runtime

#### Scenario: Variable RenderObject with N children
- **GIVEN** a RenderObject that accepts any number of children (e.g., RenderFlex, RenderStack)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBox<Variable>` or `SliverRender<Variable>`
- **AND** `ctx.children.iter()` SHALL provide iterator over all children
- **AND** child count can be zero, one, or many

### Requirement: RenderBox Protocol
RenderObjects using BoxProtocol SHALL implement layout, paint, and optionally hit_test methods with appropriate context types.

#### Scenario: Layout with BoxConstraints
- **GIVEN** a RenderBox implementation
- **WHEN** `layout()` is called
- **THEN** it SHALL receive `LayoutContext<'_, T, A, BoxProtocol>`
- **AND** `ctx.constraints` SHALL be `BoxConstraints` (min/max width/height)
- **AND** it SHALL return `Size` (width, height)
- **AND** it MAY call `ctx.layout_child()` for each child with BoxConstraints

#### Scenario: Paint to Canvas
- **GIVEN** a RenderBox implementation
- **WHEN** `paint()` is called
- **THEN** it SHALL receive `PaintContext<'_, T, A>`
- **AND** `ctx.offset` SHALL specify position in parent coordinates
- **AND** `ctx.canvas()` SHALL provide drawing surface
- **AND** it MAY call `ctx.paint_child()` for each child with Offset

#### Scenario: Hit testing for pointer events
- **GIVEN** a RenderBox implementation
- **WHEN** `hit_test()` is called (optional override)
- **THEN** it SHALL receive `HitTestContext<'_, T, A, BoxProtocol>`
- **AND** `ctx.position` SHALL specify hit location in local coordinates
- **AND** it SHALL return `bool` indicating if hit
- **AND** it MAY call `ctx.add_to_result()` to register hit

### Requirement: SliverRender Protocol
RenderObjects using SliverProtocol SHALL implement layout, paint, and optionally hit_test methods for viewport-aware scrollable content.

#### Scenario: Layout with SliverConstraints
- **GIVEN** a SliverRender implementation
- **WHEN** `layout()` is called
- **THEN** it SHALL receive `LayoutContext<'_, T, A, SliverProtocol>`
- **AND** `ctx.constraints` SHALL be `SliverConstraints` (scroll state, viewport info)
- **AND** it SHALL return `SliverGeometry` (scroll/paint/layout extents, visibility)
- **AND** it MAY call `ctx.layout_child()` for each child with SliverConstraints

#### Scenario: Sliver geometry transformation
- **GIVEN** a SliverRender that modifies geometry (e.g., RenderSliverPadding)
- **WHEN** calculating layout
- **THEN** it SHALL transform incoming constraints for child
- **AND** it SHALL transform outgoing geometry from child
- **AND** scroll_extent SHALL include modifications (e.g., padding)
- **AND** paint_extent SHALL be clamped to remaining_paint_extent

#### Scenario: Protocol bridge (Box to Sliver)
- **GIVEN** RenderSliverToBoxAdapter
- **WHEN** layout is called with SliverConstraints
- **THEN** it SHALL convert to BoxConstraints for Box child
- **AND** it SHALL layout Box child with converted constraints
- **AND** it SHALL convert Box Size to SliverGeometry
- **AND** scroll_extent SHALL equal main-axis size of Box child

### Requirement: RenderBoxProxy Pattern
RenderObjects that pass through all layout and paint operations unchanged SHALL use the RenderBoxProxy trait for zero-boilerplate implementation.

#### Scenario: Pure semantic wrapper
- **GIVEN** a RenderObject that only adds semantics (e.g., RenderSemantics)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBoxProxy` marker trait
- **AND** it SHALL automatically get `RenderBox<Single>` implementation
- **AND** `proxy_layout()` default SHALL pass constraints to child unchanged
- **AND** `proxy_paint()` default SHALL paint child at same offset

#### Scenario: Custom proxy with logging
- **GIVEN** a RenderObject that logs but doesn't modify layout/paint
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderBoxProxy`
- **AND** it MAY override `proxy_layout()` to add logging before `ctx.proxy()`
- **AND** it MAY override `proxy_paint()` to add logging before `ctx.proxy()`
- **AND** it SHALL NOT modify constraints or offset

### Requirement: RenderSliverProxy Pattern
RenderObjects that pass through sliver protocol operations unchanged SHALL use the RenderSliverProxy trait for zero-boilerplate implementation.

#### Scenario: Sliver opacity wrapper
- **GIVEN** RenderSliverOpacity that modifies paint but not layout
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderSliverProxy`
- **AND** it SHALL automatically get `SliverRender<Single>` implementation
- **AND** `proxy_layout()` default SHALL pass constraints to child unchanged
- **AND** `proxy_paint()` MAY be overridden to apply opacity layer
- **AND** geometry SHALL be returned unchanged from child

#### Scenario: Pure sliver pass-through
- **GIVEN** a sliver that only affects hit testing (e.g., RenderSliverIgnorePointer)
- **WHEN** implementing the render object
- **THEN** it SHALL implement `RenderSliverProxy`
- **AND** it SHALL override `proxy_hit_test()` only
- **AND** layout and paint SHALL use default proxy implementations

## ADDED Requirements

### Requirement: Sliver Object Inventory
The framework SHALL provide 26 sliver render objects covering all common scrollable layouts.

#### Scenario: Single-child slivers implemented
- **WHEN** building scrollable content
- **THEN** the following 10 Single slivers SHALL be available:
  - RenderSliverOpacity (proxy)
  - RenderSliverAnimatedOpacity (proxy)
  - RenderSliverIgnorePointer (proxy)
  - RenderSliverOffstage (proxy)
  - RenderSliverConstrainedCrossAxis (proxy)
  - RenderSliverToBoxAdapter (manual)
  - RenderSliverPadding (manual)
  - RenderSliverFillRemaining (manual)
  - RenderSliverEdgeInsetsPadding (manual)
  - RenderSliverOverlapAbsorber (manual)

#### Scenario: Variable-child slivers implemented
- **WHEN** building complex scrollable layouts
- **THEN** the following 16 Variable slivers SHALL be available:
  - RenderSliverList (lazy-loading list)
  - RenderSliverFixedExtentList (fixed-height items)
  - RenderSliverPrototypeExtentList (prototype-based sizing)
  - RenderSliverGrid (grid layout)
  - RenderSliverFillViewport (viewport-filling items)
  - RenderSliverAppBar (collapsing app bar)
  - RenderSliverPersistentHeader (sticky header)
  - RenderSliverFloatingPersistentHeader (floating header)
  - RenderSliverPinnedPersistentHeader (pinned header)
  - RenderSliverMainAxisGroup (main-axis grouping)
  - RenderSliverCrossAxisGroup (cross-axis grouping)
  - RenderSliverMultiBoxAdaptor (base for lists)
  - RenderSliverSafeArea (safe area insets)
  - RenderSliver (base trait)
  - RenderAbstractViewport (abstract viewport)
  - RenderShrinkWrappingViewport (shrink-wrap viewport)

### Requirement: RenderParagraph Arity Correction
RenderParagraph SHALL be classified as Variable arity to support inline children (WidgetSpan).

#### Scenario: RichText with inline widgets
- **GIVEN** a RichText with WidgetSpan inline children
- **WHEN** RenderParagraph is laid out
- **THEN** it SHALL implement `RenderBox<Variable>`
- **AND** it SHALL iterate `ctx.children.iter()` for inline children
- **AND** it SHALL position each inline child within text flow
- **AND** it SHALL paint inline children at calculated offsets

#### Scenario: Plain text without inline children
- **GIVEN** a RichText with only text spans (no WidgetSpan)
- **WHEN** RenderParagraph is laid out
- **THEN** `ctx.children.iter()` SHALL return empty iterator
- **AND** layout SHALL proceed as text-only rendering
- **AND** performance SHALL be equivalent to Leaf rendering

### Requirement: Variable Box Object Inventory
The framework SHALL provide 6 additional Variable box render objects for multi-child layouts.

#### Scenario: Complex multi-child layouts available
- **WHEN** building complex non-scrollable layouts
- **THEN** the following Variable box objects SHALL be available:
  - RenderFlow (custom delegate layout)
  - RenderTable (table layout with cell spanning)
  - RenderListWheelViewport (3D wheel picker)
  - RenderCustomMultiChildLayoutBox (custom delegate)
  - RenderOverflowIndicator (debug visualization)
  - RenderViewport (sliver container)

### Requirement: Migration Documentation
All RenderObject migration patterns SHALL be documented in CLAUDE.md and docs/plan.md.

#### Scenario: Proxy pattern examples documented
- **WHEN** developer creates new proxy render object
- **THEN** CLAUDE.md SHALL contain RenderBoxProxy example
- **AND** CLAUDE.md SHALL contain RenderSliverProxy example
- **AND** examples SHALL show both default and custom implementations

#### Scenario: Sliver protocol guide available
- **WHEN** developer implements custom sliver
- **THEN** docs SHALL contain constraint transformation pattern
- **AND** docs SHALL contain geometry aggregation pattern
- **AND** docs SHALL explain scroll_extent vs paint_extent vs layout_extent
- **AND** docs SHALL show protocol bridge example (Box ↔ Sliver)

#### Scenario: Migration checklist complete
- **WHEN** reviewing migration status
- **THEN** docs/plan.md SHALL list all 82 objects
- **AND** checklist SHALL show 82/82 (100%) migrated
- **AND** each phase SHALL be marked complete
- **AND** proxy vs manual classification SHALL be documented

## REMOVED Requirements

None. This change extends existing capabilities without deprecating any.

## RENAMED Requirements

None. All requirement names remain unchanged.
