# Rendering API Specification Delta

## ADDED Requirements

### Requirement: Arity System with Compile-Time Child Count Validation

The rendering system SHALL provide a compile-time arity system that validates child counts at type-definition time, with optional debug-only runtime validation for development safety.

#### Scenario: Leaf render object rejects children at compile time
- **WHEN** defining a Leaf render type with zero children
- **THEN** attempting to add children produces a compile error (type mismatch in push_child)
- **AND** no runtime overhead in release builds

#### Scenario: Optional render object accepts 0 or 1 child
- **WHEN** defining an Optional render type
- **THEN** the system allows up to 1 child via push_child
- **AND** OptionalChild accessor provides get(), is_some(), map() methods like Option<T>
- **AND** attempting to add a second child panics in debug builds (zero cost in release)

#### Scenario: Single render object requires exactly 1 child
- **WHEN** defining a Single render type
- **THEN** the system requires exactly 1 child
- **AND** FixedChildren<1> accessor provides single() method for compile-time safe access
- **AND** omitting the child or adding extra children panics in debug builds

#### Scenario: Variable render object accepts any number of children
- **WHEN** defining a Variable render type
- **THEN** the system accepts any number of children
- **AND** SliceChildren accessor provides iter(), get(i), first(), last() methods
- **AND** no validation overhead (Variable arity always valid)

#### Scenario: AtLeast<N> render object enforces minimum children
- **WHEN** defining an AtLeast<N> render type with N ≥ 1
- **THEN** the system requires at least N children
- **AND** SliceChildren accessor provides iteration methods
- **AND** adding fewer than N children or committing with fewer than N panics in debug builds

#### Scenario: Arity validation with debug_assert zero cost in release
- **WHEN** building a release binary
- **THEN** all debug_assert! validation for arity is compiled away
- **AND** performance is identical to manual child management
- **AND** benchmarks show ≤5% overhead in debug builds (acceptable for development)

### Requirement: Generic Render Trait Parameterized by Arity

The rendering system SHALL provide a generic Render<A: Arity> trait that works with all arity types and requires type parameters to be satisfied at compile time.

#### Scenario: Render trait accepts generic arity parameter
- **WHEN** implementing Render<Leaf> for RenderText
- **THEN** the trait provides layout(ctx: &mut BoxLayoutContext<'_, Leaf>) -> Size
- **AND** the context provides typed children access via ctx.children()
- **AND** no unsafe code or downcasting required

#### Scenario: Children accessor is automatically typed based on arity
- **WHEN** implementing Render<Single>
- **THEN** ctx.children() returns FixedChildren<'_, 1>
- **AND** FixedChildren<'_, 1> provides single() -> ElementId method
- **AND** calling get() or iter() produces compile error (method doesn't exist)

#### Scenario: Render trait has default debug_name implementation
- **WHEN** implementing Render<A>
- **THEN** debug_name() defaults to std::any::type_name::<Self>()
- **AND** can be overridden for custom display names
- **AND** debug names appear in tracing logs and error messages

#### Scenario: Downcast to concrete render type for mutation
- **WHEN** needing to mutate a specific render object
- **THEN** use downcast_rs to cast Box<dyn DynRenderObject> to concrete type
- **AND** panics if cast fails (type mismatch, shouldn't happen in correct code)
- **AND** zero-cost abstraction (single vtable pointer check)

### Requirement: SliverRender Trait for Scrollable Content

The rendering system SHALL provide a SliverRender<A: Arity> trait for scrollable/viewport-aware content with identical arity guarantees as Render<A>.

#### Scenario: SliverRender follows same arity pattern as Render
- **WHEN** implementing SliverRender<Variable>
- **THEN** layout(ctx: &mut SliverLayoutContext<'_, Variable>) -> SliverGeometry
- **AND** ctx.children() returns SliceChildren with all iteration methods
- **AND** type safety enforced at compile time

#### Scenario: Sliver context provides box and sliver child layout methods
- **WHEN** in a sliver layout context
- **THEN** can call ctx.layout_sliver_child(id, constraints) -> SliverGeometry
- **AND** can call ctx.layout_box_child(id, constraints) -> Size
- **AND** mixing protocols (Box render in Sliver context) returns proper error

### Requirement: Context Types with HasTypedChildren Trait

The rendering system SHALL provide context types (BoxLayoutContext, BoxPaintContext, BoxHitTestContext, etc.) that implement HasTypedChildren<A> for type-safe child access.

#### Scenario: Layout context provides children typed by arity
- **WHEN** implementing Render<Pair> layout
- **THEN** ctx.children() returns FixedChildren<'_, 2>
- **AND** FixedChildren<'_, 2> provides pair() -> (ElementId, ElementId)
- **AND** type system prevents accessing single(), triple(), or iter()

#### Scenario: Paint context follows same HasTypedChildren trait
- **WHEN** implementing Render<A> paint
- **THEN** ctx.children() returns A::Children<'_>
- **AND** HasTypedChildren<A> automatically provides the right accessor type
- **AND** paint_child(id, offset) -> Canvas works for all arities

#### Scenario: Hit test context includes result accumulator
- **WHEN** implementing hit_test with children
- **THEN** ctx.result is mutable HitTestResult reference
- **AND** child hit_test calls via ctx.hit_test_child(id, position) -> bool
- **AND** parent accumulates hit results in ctx.result

### Requirement: Type Erasure via DynRenderObject Trait

The rendering system SHALL provide a DynRenderObject trait that erases type information while preserving protocol and arity invariants via RenderElement storage.

#### Scenario: DynRenderObject does not include protocol or arity info
- **WHEN** implementing DynRenderObject for a specific Render<A>
- **THEN** dyn_layout(tree, children: &[ElementId], constraints) -> DynGeometry
- **AND** dyn_paint(tree, children: &[ElementId], offset) -> Canvas
- **AND** dyn_hit_test(tree, children: &[ElementId], position, result) -> DynHitTestResult
- **AND** protocol/arity stored in RenderElement, not in DynRenderObject

#### Scenario: Safe wrappers implement DynRenderObject without unsafe
- **WHEN** BoxRenderObjectWrapper<A, R> is created with Render<A> + Send + Sync
- **THEN** wrapper implements DynRenderObject safely
- **AND** no unsafe code in wrapper (only debug_assert validation)
- **AND** arity validation happens at Box creation time and dyn_layout time

#### Scenario: Debug-only arity validation in wrappers
- **WHEN** dyn_layout is called with mismatched children count
- **THEN** debug_assert! checks A::validate_count(children.len())
- **AND** panic in debug builds with clear message including element name and expected arity
- **AND** zero cost in release builds (assertion compiled away)

### Requirement: RenderState Unified Enum for Both Protocols

The rendering system SHALL provide a unified RenderState enum supporting both Box and Sliver protocols with protocol-specific data and common flags.

#### Scenario: RenderState stores protocol-specific geometry
- **WHEN** creating RenderState for Box protocol
- **THEN** RenderState::Box variant stores Size and BoxConstraints
- **AND** accessing via state.size() returns Option<Size>
- **AND** calling state.as_box_mut() returns mutable BoxRenderState

#### Scenario: RenderState provides common flag interface
- **WHEN** marking needs_layout or needs_paint
- **THEN** state.flags().mark_needs_layout() works for both Box and Sliver
- **AND** state.flags().needs_layout() returns bool
- **AND** flags stored in AtomicRenderFlags (lock-free atomic operations)

#### Scenario: RenderState protocol accessor
- **WHEN** needing to know the protocol
- **THEN** state.protocol() returns LayoutProtocol::Box or ::Sliver
- **AND** protocol() is inline always (zero cost)

---

## MODIFIED Requirements

### Requirement: Layout and Paint Pipeline Integration

The layout and paint phases SHALL accept elements with unified RenderState and support both Box and Sliver protocols transparently.

#### Scenario: Layout phase handles both protocols
- **WHEN** flush_layout() processes an element
- **THEN** it passes DynConstraints::Box or ::Sliver to render object
- **AND** receives DynGeometry back and stores in RenderState
- **AND** no protocol-specific branching in hot path (handled by traits)

#### Scenario: Paint phase generates canvas layers
- **WHEN** flush_paint() processes an element
- **THEN** it calls render_object.dyn_paint() with offset
- **AND** receives Canvas back
- **AND** flags cleared via state.flags().clear_needs_paint()

#### Scenario: Hit testing unified across protocols
- **WHEN** hit_test() is called on an element
- **THEN** it passes position and mutable result to render object
- **AND** receives DynHitTestResult (Box(bool) or Sliver(SliverHitTestResult))
- **AND** parent accumulates results correctly

---

## REMOVED Requirements

### Requirement: Separate LeafRender, SingleRender, MultiRender Traits

**Reason:** Replaced by generic Render<A: Arity> trait with identical semantics but added compile-time type safety.

**Migration:**
- Find all impl LeafRender → migrate to impl Render<Leaf>
- Find all impl SingleRender → migrate to impl Render<Single>
- Find all impl MultiRender → migrate to impl Render<Variable>
- Update context types from LayoutContext → BoxLayoutContext<A>
- Update signatures from RenderNode variant → unified RenderElement

### Requirement: Manual Arity Validation in Render Objects

**Reason:** Compile-time arity system and debug_assert validation eliminate need for manual checks.

**Migration:**
- Remove arity() method implementations
- Remove validate_arity() calls from widget code
- Rely on compile-time type signatures for safety
- In debug builds, assertions catch mismatches automatically
