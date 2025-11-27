# Technical Design: RenderObject Migration

## Context

FLUI's rendering system migrated from a three-trait pattern (LeafRender, SingleRender, MultiRender) to a unified RenderBox<Arity> system. Phase 3-5 completed 34 Single arity objects successfully. This design covers migrating the remaining 28 objects across Leaf, Variable, and all Sliver protocols.

### Current State
- **Completed**: 54/82 objects (66%)
  - Phase 3: 10 Single Layout objects ✅
  - Phase 4: 14 Single Effects objects ✅
  - Phase 5: 10 Single Interaction/Semantics objects ✅
- **Remaining**: 28 objects (34%)
  - 2 Leaf (1 misclassified)
  - 6 Variable Box
  - 20 Sliver (10 Single + 16 Variable - 6 misclassified as Variable)

### Constraints
- Must maintain existing external APIs (no breaking changes)
- Must follow Flutter's proxy pattern classification exactly
- Must build incrementally (phase-by-phase validation)
- Must align with FLUI architectural principles

## Goals / Non-Goals

### Goals
1. **100% Migration**: All 82 objects using new RenderBox/SliverRender API
2. **Proxy Pattern Adoption**: 5 sliver objects using RenderSliverProxy (zero-boilerplate)
3. **Correct Classification**: Fix RenderParagraph misclassification (Leaf → Variable)
4. **Incremental Validation**: Each phase compiles before moving forward
5. **Documentation**: Complete migration guide and pattern examples

### Non-Goals
1. **API Changes**: Not changing external widget APIs
2. **Performance Optimization**: Not rewriting algorithms (only API migration)
3. **New Features**: Not adding new render objects
4. **Protocol Changes**: Not modifying BoxProtocol or SliverProtocol
5. **Test Rewrite**: Existing tests should continue passing

## Decisions

### Decision 1: Proxy Pattern Classification

**Decision**: Use Flutter's RenderProxyBox/RenderProxySliver as authoritative source for proxy classification.

**Rationale**:
- Flutter has battle-tested patterns over 8+ years
- Phase 3-5 showed NO proxy usage for Single Box objects (all custom logic)
- Flutter's [proxy_sliver.dart](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_sliver.dart) explicitly lists 5 proxy slivers
- Consistency with ecosystem reduces cognitive load

**Alternatives Considered**:
- **Option A**: Aggressive proxy usage (use proxy wherever possible)
  - Rejected: Phase 3-5 evidence shows most objects need custom layout/paint
- **Option B**: No proxy usage (manual implementation for all)
  - Rejected: Wastes boilerplate for semantic-only wrappers
- **Option C**: Hybrid (proxy for semantics, manual for layout/paint) ✅ CHOSEN
  - Matches Flutter exactly
  - Clear decision criteria: proxy = semantic/interaction wrapper without layout changes

**Evidence from Flutter**:
```rust
// Proxy objects (from proxy_sliver.dart):
RenderSliverOpacity           // Opacity without layout change
RenderSliverIgnorePointer     // Hit test without layout change
RenderSliverOffstage          // Visibility without layout change
RenderSliverAnimatedOpacity   // Animated opacity
RenderSliverConstrainedCrossAxis  // Cross-axis constraint only
RenderSliverSemanticsAnnotations  // Pure semantics

// Non-proxy objects (custom layout/geometry):
RenderSliverPadding          // Transforms SliverGeometry
RenderSliverToBoxAdapter     // Protocol bridge (Box → Sliver)
RenderSliverFillRemaining    // Custom geometry calculation
```

### Decision 2: RenderParagraph Reclassification

**Decision**: Reclassify RenderParagraph from Leaf to Variable arity.

**Rationale**:
- Flutter's RenderParagraph uses `ContainerRenderObjectMixin<RenderBox, TextParentData>`
- Can contain inline children (WidgetSpan in RichText)
- Current Leaf classification is incorrect and will cause runtime errors

**Migration Path**:
1. Change trait bound: `impl RenderBox<Leaf>` → `impl RenderBox<Variable>`
2. Update layout to handle inline children: `ctx.children.iter()`
3. Update paint to render inline children at calculated positions
4. Add tests for RichText with WidgetSpan inline children

**Impact**:
- Breaking for users relying on Leaf assumption (likely none)
- Enables proper RichText support with inline widgets

### Decision 3: Phased Migration Order

**Decision**: Migrate in order of increasing complexity with foundation-first approach.

**Phase Order**:
1. **Phase 1**: Quick Wins (6 objects) - High confidence, low risk
   - 5 Sliver Proxy (one-line implementations)
   - 1 Leaf verification (RenderEditableLine)
2. **Phase 2**: Sliver Single Manual (5 objects) - Moderate complexity
   - Well-defined geometry transformations
   - Reference: Flutter source implementations
3. **Phase 3**: Critical Foundation (3 objects) - High risk, high value
   - RenderSliver, RenderSliverMultiBoxAdaptor, RenderSliverList
   - Foundation for all subsequent slivers
4. **Phase 4-7**: Remaining objects - Build on foundation

**Rationale**:
- Early wins build momentum and validate patterns
- Foundation objects unlock dependent implementations
- Complexity increases gradually
- Each phase provides working incremental value

**Alternatives Considered**:
- **By category** (all layout, all effects, etc.)
  - Rejected: Violates dependency order (e.g., lists need MultiBoxAdaptor)
- **By file location** (all in sliver/, all in layout/)
  - Rejected: Mixes complexity levels, harder to validate
- **All at once** (big-bang migration)
  - Rejected: High risk, no incremental validation

### Decision 4: Sliver Geometry Pattern

**Decision**: Use constraint transformation + geometry aggregation pattern for all slivers.

**Pattern**:
```rust
impl SliverRender<Single> for RenderSliverPadding {
    fn layout(...) -> SliverGeometry {
        // 1. Transform incoming constraints
        let child_constraints = transform_constraints(ctx.constraints, self.padding);

        // 2. Layout child
        let child_geometry = ctx.layout_child(child_id, child_constraints);

        // 3. Transform outgoing geometry
        transform_geometry(child_geometry, self.padding, ctx.constraints)
    }
}
```

**Rationale**:
- Consistent across all geometry-modifying slivers
- Easy to understand and debug
- Mirrors Flutter's implementation
- Separates concerns: constraint logic vs geometry logic

**Examples**:
- **RenderSliverPadding**: Adds padding to scroll_extent and paint_extent
- **RenderSliverToBoxAdapter**: Converts Box size to Sliver geometry
- **RenderSliverFillRemaining**: Calculates geometry to fill remaining viewport

### Decision 5: Module Organization

**Decision**: Uncomment modules incrementally after each phase, not at the end.

**Rationale**:
- Immediate compilation feedback
- Catch integration issues early
- Enables incremental testing
- Reduces risk of merge conflicts

**Process**:
1. Implement objects in phase
2. Uncomment module declarations in `mod.rs`
3. Uncomment re-exports in parent `mod.rs`
4. Run `cargo build -p flui_rendering`
5. Fix any compilation errors before next phase

**Checkpoints**:
- Phase 1: 5 proxy slivers enabled
- Phase 2: All 10 Sliver Single enabled
- Phase 3: Foundation slivers enabled
- Phases 4-7: Incrementally enable remaining objects

## Risks / Trade-offs

### Risk 1: RenderParagraph Complexity
**Risk**: Inline children support may require significant TextPainter changes.

**Likelihood**: Medium
**Impact**: High (blocks text rendering)

**Mitigation**:
1. Study Flutter's TextPainter and InlineSpan architecture
2. Implement minimal inline child support first (positioning only)
3. Defer advanced features (baseline alignment, wrapping around children)
4. Add comprehensive tests for WidgetSpan positioning
5. Consider temporary feature flag if implementation takes >2 weeks

**Fallback**:
- Keep RenderParagraph as Leaf with assertion `children.is_empty()`
- Document limitation: "Inline children not yet supported"
- Revisit in separate change proposal

### Risk 2: Sliver Geometry Bugs
**Risk**: Incorrect geometry calculations cause scroll glitches.

**Likelihood**: Medium-High
**Impact**: High (broken scrolling UX)

**Mitigation**:
1. Port logic directly from Flutter (don't rewrite algorithms)
2. Add geometry validation helpers (assert scroll_extent >= paint_extent, etc.)
3. Create visual diff tests comparing FLUI vs Flutter output
4. Test edge cases: zero extents, negative scroll offsets, overlapping slivers
5. Implement scrolling debugger visualization

**Indicators**:
- Slivers not visible when they should be
- Scroll position jumps unexpectedly
- Infinite scroll extent
- Paint extent exceeds remaining paint extent

### Risk 3: Viewport Scroll Coordination
**Risk**: RenderViewport implementation is the most complex, high chance of subtle bugs.

**Likelihood**: High
**Impact**: Critical (blocks all scrollable content)

**Mitigation**:
1. Implement in Phase 3 (Critical Foundation) not Phase 7
2. Build RenderSliverMultiBoxAdaptor first (shared logic)
3. Start with simplest viewport: single sliver, no caching
4. Add incremental features: multiple slivers, cache extent, reverse scrolling
5. Extensive test coverage: unit + integration + visual tests
6. Consider parallel implementation by two developers for cross-validation

**Fallback**:
- Use simplified viewport with limited features
- Document limitations clearly
- Mark advanced features as "experimental"

### Risk 4: Compilation Errors at Scale
**Risk**: Uncommenting all modules reveals unexpected trait conflicts or orphan impl issues.

**Likelihood**: Low-Medium
**Impact**: Medium (delays completion)

**Mitigation**:
1. Incremental uncommenting (1 phase at a time)
2. Run `cargo clippy` after each phase
3. Use `cargo build -p flui_rendering` not full workspace (faster feedback)
4. Keep phase scope small (3-6 objects per phase)

**Recovery**:
- Re-comment problematic modules
- Fix issues in isolation
- Re-enable once fixed

## Migration Plan

### Pre-Migration Checklist
- [ ] All Phase 3-5 objects passing tests
- [ ] CI pipeline green
- [ ] No pending rendering PRs (minimize conflicts)
- [ ] Migration tracking spreadsheet created

### Migration Sequence

**Week 1: Foundation**
- Days 1-2: Phase 1 (Quick Wins - 6 objects)
- Days 3-5: Phase 2 (Sliver Single Manual - 5 objects)

**Week 2: Critical Path**
- Days 1-3: Phase 3 (Critical Foundation - 3 objects)
- Days 4-5: Phase 4 (Variable Box - 3 objects)

**Week 3: Essential Features**
- Days 1-3: Phase 5 (Essential Slivers - 3 objects)
- Days 4-5: Testing and documentation

**Week 4: Advanced Features**
- Days 1-3: Phase 6 (Complex Variable Box - 3 objects)
- Days 4-5: Phase 7 start (Advanced Slivers - 5/13 objects)

**Week 5: Completion**
- Days 1-4: Phase 7 complete (remaining 8 objects)
- Day 5: Final validation, documentation, archive

### Rollback Plan

If critical bug discovered:
1. Identify affected phase
2. Revert commits for that phase
3. Re-comment modules in `mod.rs`
4. Verify build passes
5. Fix bug in isolation
6. Re-apply phase with fix

### Post-Migration Validation

**Compilation**:
```bash
cargo build --workspace --all-features
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps
```

**Testing**:
```bash
cargo test --workspace
cargo test -p flui_rendering -- --nocapture
cargo bench -p flui_rendering  # Performance regression check
```

**Visual Testing**:
- Run all examples in `examples/`
- Compare screenshots with pre-migration baseline
- Test scrolling performance (60 FPS maintained)

## Open Questions

### Q1: RenderParagraph Inline Children Strategy
**Question**: Implement full inline child support immediately or defer?

**Options**:
- A) Full implementation (baseline alignment, wrapping, etc.) - 2-3 weeks
- B) Minimal support (fixed positioning only) - 3-5 days
- C) Defer to separate proposal (keep as Leaf with assertion) - 0 days

**Recommendation**: Option B (minimal support)
- Unblocks migration
- Provides basic RichText functionality
- Can enhance incrementally

### Q2: Test Strategy for Slivers
**Question**: How to validate sliver geometry calculations comprehensively?

**Options**:
- A) Unit tests only (fast but incomplete)
- B) Visual regression tests (comprehensive but slow)
- C) Property-based tests (thorough but complex setup)
- D) Hybrid: Unit + selective visual tests

**Recommendation**: Option D
- Unit tests for geometry math
- Visual tests for 5-10 critical scenarios (list scrolling, app bar collapse, etc.)
- Enables fast iteration with confidence

### Q3: Documentation Format
**Question**: Where to document sliver protocol patterns?

**Options**:
- A) In CLAUDE.md (user-facing)
- B) In crates/flui_rendering/docs/ (developer-facing)
- C) In code comments (implementation-facing)
- D) All three

**Recommendation**: Option D
- CLAUDE.md: High-level patterns for users
- docs/: Detailed protocol guide for contributors
- Comments: Implementation details for maintainers

## Success Metrics

1. **Coverage**: 82/82 objects migrated (100%)
2. **Compilation**: Zero errors, zero warnings
3. **Performance**: No regressions (benchmark suite)
4. **Tests**: All existing tests passing + new sliver tests
5. **Documentation**: Migration guide + sliver protocol guide complete
6. **Timeline**: Completed within 5 weeks
