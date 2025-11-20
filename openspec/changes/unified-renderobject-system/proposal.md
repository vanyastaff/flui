# Change: Unified RenderObject Architecture with Type-Safe Arity System

## Why

FLUI's current render system uses separate trait hierarchies (LeafRender, SingleRender, MultiRender) that lack type-safe child validation at compile time. This leads to runtime panics when render objects receive incorrect child counts, and forces boilerplate arity validation code in every widget.

The unified RenderObject system introduces a **type-safe arity system** where child count validation happens at **compile time via generic parameters**, with optional runtime validation in debug builds (zero cost in release). This eliminates entire classes of bugs while maintaining peak performance.

**Problem:**
- Runtime panics from child count mismatches (should be compile-time errors)
- Boilerplate arity validation in every widget
- No first-class support for optional children (0 or 1)
- Type erasure loses arity information, requiring error-prone runtime checks
- Lock ordering complexities not documented in thread-safe design

**Opportunity:**
- Move validation to compile time with zero runtime cost
- Support all arity patterns (Leaf, Optional, Single, Pair, Triple, Variable, AtLeast<N>)
- Type-safe children accessors (no unwrap() needed)
- Single source of truth for protocol/arity in RenderElement
- Documented thread-safety guarantees with explicit lock ordering

## What Changes

**BREAKING CHANGES:**
- Replace LeafRender, SingleRender, MultiRender traits with generic Render<A: Arity> trait
- Migrate all render objects to new Render<A>/SliverRender<A> API
- Change RenderElement layout/paint signatures to use DynConstraints/DynGeometry
- Centralize children management with transactional API

**Non-Breaking Additions:**
- New arity types: Leaf, Optional, Single, Pair, Triple, Exact<N>, AtLeast<N>, Variable
- New children accessors with compile-time type safety
- New context types: BoxLayoutContext<A>, BoxPaintContext<A>, BoxHitTestContext<A>, etc.
- New RenderState enum supporting Box and Sliver protocols
- New DynRenderObject trait for type erasure
- New ElementTree methods: request_layout(), request_paint()
- New RenderElement constructors for each protocol/arity combination

**Affected Specs:**
- `rendering-api` - New Render<A> and SliverRender<A> traits
- `element-tree` - RenderElement type erasure and children management

## Impact

**Affected Code:**
- `crates/flui_rendering/src/` - All render object implementations
- `crates/flui_core/src/element/render_element.rs` - New RenderElement structure
- `crates/flui_widgets/src/` - All widget implementations
- `crates/flui_rendering/src/objects/` - Render object base classes

**Migration Path:**
- Phase 1: Implement new arity system and traits in flui_core
- Phase 2: Implement RenderElement with type erasure
- Phase 3: Migrate all render objects (leaf widgets first, then composite widgets)
- Phase 4: Update ElementTree scheduling API
- Phase 5: Testing and benchmarking
- Phase 6: Documentation and examples

**Performance:**
- No regression expected (debug_assert validation zero cost in release)
- Potential improvement from better type information enabling compiler optimizations
- Lock-free flag checks via atomic operations remain unchanged

**Timeline:**
- Implementation: ~3-4 weeks (distributed across crates)
- Testing/Benchmarking: ~1 week
- Integration/Polish: ~1 week

**Version Bump:**
- **BREAKING:** Increment to v0.7.0
- Deprecation period: None (clean architectural break)
- Migration guide included in documentation

## Acceptance Criteria

- [x] All render objects migrated to Render<A>/SliverRender<A> traits
- [x] Benchmarks show ≤10% perf regression (expect neutral or improvement)
- [x] No unsafe code in wrapper implementations
- [x] Documentation updated with API reference and examples
- [x] Thread safety invariants documented with explicit lock ordering
- [x] Arity validation in debug mode only (zero cost in release)
- [x] 100% test coverage for new arity system
- [x] Loom deadlock prevention tests for lock ordering
