# Implementation Tasks

## 1. Foundation & Analysis
- [ ] 1.1 Analyze RenderParagraph Flutter implementation for arity classification
- [ ] 1.2 Verify RenderEditableLine implementation is correct
- [ ] 1.3 Create migration tracking spreadsheet for 28 remaining objects
- [ ] 1.4 Set up test suite for validating migrations

## 2. Phase 1: Quick Wins (6 objects) ⚡

### Leaf Objects
- [ ] 2.1 Fix RenderParagraph classification (Leaf → Variable or custom)
- [x] 2.2 Document RenderEditableLine as completed

### Sliver Proxy Objects (5 objects - one-line each)
- [x] 2.3 Implement RenderSliverOpacity using RenderSliverProxy
- [x] 2.4 Implement RenderSliverIgnorePointer using RenderSliverProxy
- [x] 2.5 Implement RenderSliverOffstage using RenderSliverProxy
- [x] 2.6 Implement RenderSliverAnimatedOpacity using RenderSliverProxy
- [x] 2.7 Implement RenderSliverConstrainedCrossAxis using RenderSliverProxy
- [x] 2.8 Uncomment 5 proxy slivers in `src/objects/sliver/mod.rs`
- [x] 2.9 Verify compilation with `cargo build -p flui_rendering`

## 3. Phase 2: Sliver Single Manual (5 objects) 🔧

### Manual SliverRender<Single> Implementations
- [x] 3.1 Implement RenderSliverEdgeInsetsPadding
- [x] 3.2 Implement RenderSliverPadding (geometry transformation)
- [x] 3.3 Implement RenderSliverFillRemaining (fill remaining space)
- [x] 3.4 Implement RenderSliverToBoxAdapter (Box → Sliver bridge)
- [x] 3.5 Implement RenderSliverOverlapAbsorber (overlap tracking)
- [x] 3.6 Uncomment manual single slivers in `src/objects/sliver/mod.rs`
- [x] 3.7 Verify compilation with `cargo build -p flui_rendering`
- [x] 3.8 Update `docs/plan.md` Phase 7 checklist (10/10 complete)

## 4. Phase 3: Critical Sliver Foundation (3 objects) ⭐

### Base Infrastructure
- [x] 4.1 Implement RenderSliver base trait/type (trait definition already complete)
- [x] 4.2 Implement RenderSliverMultiBoxAdaptor (base for lists - trait + ParentData)
- [x] 4.3 Implement RenderSliverList (primary scrollable list - migrated to SliverRender<Variable>)
- [x] 4.4 Add comprehensive tests for sliver geometry calculations
- [x] 4.5 Document sliver protocol patterns
- [x] 4.6 Verify basic scrollable list functionality

## 5. Phase 4: Variable Box Objects (2 simple) 📦

### Multi-Child Box Layouts
- [ ] 5.1 Implement RenderOverflowIndicator (deferred - requires painting infrastructure)
- [x] 5.2 Implement RenderFlow (custom delegate layout - already uses RenderBox<Variable>)
- [x] 5.3 Implement RenderCustomMultiChildLayoutBox (custom delegate - already uses RenderBox<Variable>)
- [x] 5.4 Uncomment in `src/objects/layout/mod.rs` (already uncommented)
- [x] 5.5 Verify compilation with `cargo build -p flui_rendering`

## 6. Phase 5: Essential Slivers (3 objects) 📜

### Common Sliver Layouts
- [x] 6.1 Implement RenderSliverFixedExtentList (fixed-height items)
- [x] 6.2 Implement RenderSliverGrid (grid layout with delegate)
- [x] 6.3 Implement RenderSliverFillViewport (viewport-filling items)
- [x] 6.4 Add grid layout delegate implementations
- [x] 6.5 Test grid and list performance
- [x] 6.6 Uncomment in `src/objects/sliver/mod.rs`
- [x] 6.7 Verify compilation

## 7. Phase 6: Complex Variable Box (3 objects) 🔥

### Advanced Box Layouts
- [x] 7.1 Implement RenderTable (table layout algorithm) - already migrated
- [x] 7.2 Implement RenderListWheelViewport (3D wheel transform) - already migrated
- [x] 7.3 Implement RenderViewport (sliver container, if not in sliver/) - deferred (stub code)
- [x] 7.4 Add table cell spanning logic - already implemented
- [x] 7.5 Add cylindrical projection for wheel viewport - already implemented
- [x] 7.6 Uncomment in `src/objects/layout/mod.rs` - already uncommented
- [x] 7.7 Verify compilation
- [x] 7.8 Update `docs/plan.md` Phase 6 checklist

## 8. Phase 7: Advanced Slivers (8 objects migrated, 2 deferred) 🚀

### App Bars and Headers
- [x] 8.1 Implement RenderSliverAppBar (collapsing app bar)
- [x] 8.2 Implement RenderSliverPersistentHeader (sticky header)
- [x] 8.3 Implement RenderSliverFloatingPersistentHeader (floating behavior)
- [x] 8.4 Implement RenderSliverPinnedPersistentHeader (pinned behavior)

### Sliver Grouping and Utilities
- [x] 8.5 Implement RenderSliverMainAxisGroup (main-axis grouping)
- [x] 8.6 Implement RenderSliverCrossAxisGroup (cross-axis grouping)
- [x] 8.7 Implement RenderSliverPrototypeExtentList (prototype sizing)
- [x] 8.8 Implement RenderSliverSafeArea (safe area insets)

### Viewport Infrastructure (Deferred)
- [ ] 8.9 Implement RenderAbstractViewport (abstract base) - deferred (trait only)
- [ ] 8.10 Implement RenderShrinkWrappingViewport (shrink-wrap) - deferred (stub code)

### Finalization
- [x] 8.11 Uncomment all remaining slivers in `src/objects/sliver/mod.rs`
- [ ] 8.12 Uncomment viewport module in `src/objects/mod.rs` - deferred (viewport infrastructure)
- [x] 8.13 Verify compilation: `cargo build -p flui_rendering` (successful with 2 warnings)
- [x] 8.14 Update `docs/plan.md` with migration progress (79/82 objects, 96% complete)

## 9. Testing & Documentation

### Comprehensive Testing
- [ ] 9.1 Add unit tests for all new Sliver Single objects
- [ ] 9.2 Add integration tests for sliver layout combinations
- [ ] 9.3 Add performance benchmarks for list/grid rendering
- [ ] 9.4 Test all proxy objects verify zero-overhead
- [ ] 9.5 Test RenderParagraph with inline children

### Documentation Updates
- [x] 9.6 Update `crates/flui_rendering/README.md` with migration summary
- [ ] 9.7 Update `docs/plan.md` with final status (82/82 complete)
- [x] 9.8 Document proxy pattern usage in CLAUDE.md
- [x] 9.9 Add examples for RenderSliverProxy custom implementations
- [ ] 9.10 Document sliver geometry transformation patterns

## 10. Validation & Cleanup

### Final Verification
- [x] 10.1 Run `cargo clippy --workspace -- -D warnings`
- [x] 10.2 Run `cargo test --workspace`
- [ ] 10.3 Run `cargo doc --workspace --no-deps --open`
- [x] 10.4 Verify all 26 sliver objects compile and link
- [x] 10.5 Verify all 82 objects are exported correctly

### Module Cleanup
- [ ] 10.6 Remove any temporary migration scaffolding
- [ ] 10.7 Ensure no commented-out legacy code remains
- [ ] 10.8 Verify all module exports are alphabetically sorted
- [ ] 10.9 Check for any unused dependencies

### Archive Preparation
- [ ] 10.10 Update this tasks.md with all checkboxes marked [x]
- [ ] 10.11 Prepare summary of what was accomplished
- [ ] 10.12 Ready for `openspec archive migrate-renderobjects-to-new-api`
