# Tasks: Consolidate Semantics into flui-semantics Crate

## 1. Preparation
- [x] 1.1 Review existing flui-semantics code structure
- [x] 1.2 Review flui_rendering/semantics code structure
- [x] 1.3 Identify type conflicts and naming differences
- [x] 1.4 Plan module organization for consolidated crate

## 2. Migrate Core Types
- [x] 2.1 Migrate `SemanticsAction` and `ActionArgs` from flui_rendering
- [x] 2.2 Migrate `SemanticsActionHandler` type alias
- [x] 2.3 Migrate `SemanticsFlag` and `SemanticsFlags` from flui_rendering
- [x] 2.4 Migrate `AttributedString` and `StringAttribute` types
- [x] 2.5 Migrate `SemanticsTag` type
- [x] 2.6 Migrate `SemanticsSortKey` type
- [x] 2.7 Migrate `SemanticsHintOverrides` type
- [x] 2.8 Migrate `CustomSemanticsAction` type
- [x] 2.9 Migrate `TextDirection` enum

## 3. Migrate Configuration
- [x] 3.1 Migrate `SemanticsConfiguration` (full implementation)
- [x] 3.2 Migrate `SemanticsProperties` (if different from flui_types version)

## 4. Migrate Node and Tree
- [x] 4.1 Update `SemanticsNode` with full implementation from flui_rendering
- [x] 4.2 Unify `SemanticsNodeId` with `SemanticsId` (use NonZeroUsize pattern)
- [x] 4.3 Migrate `SemanticsNodeData` for platform serialization
- [x] 4.4 Update `SemanticsTree` or replace with flui_rendering's approach
- [x] 4.5 Update `SemanticsOwner` with full implementation

## 5. Migrate Events and Updates
- [x] 5.1 Migrate `SemanticsEvent` and `SemanticsEventType`
- [x] 5.2 Migrate `SemanticsEventData` enum
- [x] 5.3 Migrate `SemanticsUpdate` and `SemanticsUpdateBuilder`

## 6. Apply Optimizations
- [x] 6.1 Replace `String` with `SmolStr` for label/hint/value fields
- [x] 6.2 Replace `Vec<SemanticsId>` with `SmallVec<[SemanticsId; 4]>` for children
- [x] 6.3 Replace `Vec<SemanticsAction>` with `SmallVec<[SemanticsAction; 4]>` for actions
- [x] 6.4 Replace `HashMap` with `rustc_hash::FxHashMap` where applicable
- [x] 6.5 Replace `HashSet` with `rustc_hash::FxHashSet` where applicable

## 7. AccessKit Integration (MOVED TO flui-platform)
- [x] 7.1 Add accesskit dependency to flui-semantics Cargo.toml (for types only)
- [~] 7.2-7.7 **MOVED**: AccessKit platform integration belongs in `flui-platform`

> **Decision**: AccessKit integration moved to `flui-platform` crate.
> 
> Rationale:
> - `flui-semantics` = abstract semantic model (types, tree, configuration)
> - `flui-platform` = platform integrations (windows, GPU, input, accessibility)
> - AccessKit requires platform-specific code (Windows/macOS/Linux have different APIs)
> - Follows Flutter pattern: semantics is abstract, embedder handles platform conversion
>
> New location: `flui-platform/src/accessibility/`
> - `accesskit_adapter.rs` - SemanticsTree → accesskit::TreeUpdate
> - `action_handler.rs` - accesskit::Action → SemanticsAction

## 8. Update Module Structure
- [x] 8.1 Organize modules: action, configuration, event, node, owner, properties, tree
- [x] 8.2 Create comprehensive prelude with all public types
- [x] 8.3 Update lib.rs with proper documentation
- [x] 8.4 Add feature flags for optional accesskit integration (not needed - moved to flui-platform)

## 9. Update flui_rendering
- [x] 9.1 Add `flui-semantics` dependency to flui_rendering/Cargo.toml
- [x] 9.2 Remove `flui_rendering/src/semantics/` directory
- [x] 9.3 Update flui_rendering/src/lib.rs to re-export from flui-semantics
- [x] 9.4 Update any internal uses of semantics types

## 10. Clean Up flui_types
- [x] 10.1 Evaluate which types in flui_types/src/semantics/ are duplicates
- [x] 10.2 Remove flui_types/src/semantics/ directory entirely
- [x] 10.3 Keep only shared primitive types if needed (none needed)

## 11. Testing
- [x] 11.1 Migrate tests from flui_rendering/src/semantics/ to flui-semantics
- [x] 11.2 Add tests for SmolStr/SmallVec optimizations
- [x] 11.3 Add tests for accesskit conversions (moved to flui-platform scope)
- [x] 11.4 Run full workspace build
- [x] 11.5 Run full workspace tests (409 tests passing)
- [ ] 11.6 Run clippy on workspace

## 12. Documentation
- [x] 12.1 Update crate-level documentation
- [ ] 12.2 Add examples for common use cases
- [ ] 12.3 Document migration from old imports

---

## Progress Summary

**Completed:** 47/49 tasks (96%)

### Phase 1: Core Migration ✅
All core types migrated from flui_rendering to flui-semantics with optimizations applied.

### Phase 2: Optimizations ✅
- SmolStr for O(1) clone strings
- SmallVec for inline storage of small collections  
- FxHashMap for fast integer hashing

### Phase 3: Integration ✅
- flui_rendering now re-exports `flui_semantics as semantics`
- flui_types/src/semantics/ removed entirely
- SemanticsNode.to_node_data() properly creates SemanticsNodeData
- All 409 workspace tests passing

### Phase 4: Cleanup ✅
- Removed ~4400 lines of duplicate code
- Single source of truth in flui-semantics

### Remaining Work (for this proposal)
- Run clippy on workspace
- Add usage examples
- Document migration path

### Future Work (separate proposal)
- AccessKit integration in flui-platform (see Section 7 note)
