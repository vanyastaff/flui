# Design: Consolidate Semantics into flui-semantics Crate

## Context

FLUI follows Flutter's five-tree architecture: View → Element → Render → Layer → Semantics. The semantics tree provides accessibility information to assistive technologies (screen readers, switch control, etc.).

Currently, semantics code is split across:
- `flui-semantics` crate (basic implementation)
- `flui_rendering/src/semantics/` (more complete implementation)
- `flui_types/src/semantics/` (shared types)

This design consolidates everything into `flui-semantics` as the single source of truth.

## Goals

- Single, comprehensive semantics crate
- Type-safe, optimized implementation
- Platform accessibility integration via accesskit
- Consistent with FLUI's ID and tree patterns

## Non-Goals

- Platform-specific accessibility code (deferred to accesskit)
- Full Flutter semantics parity (iterative approach)
- Breaking flui_types public API unnecessarily

## Decisions

### Decision 1: Use flui-semantics as the canonical crate

**What**: All semantics types live in `flui-semantics`. `flui_rendering` depends on it.

**Why**: 
- Follows FLUI's layered architecture (foundation → framework → rendering)
- Semantics is a cross-cutting concern used by multiple crates
- Cleaner dependency graph

**Alternatives considered**:
- Keep in flui_rendering: Rejected - semantics is not rendering-specific
- Keep in flui_types: Rejected - too much logic for a "types" crate

### Decision 2: Use SmolStr for text fields

**What**: Replace `String` with `smol_str::SmolStr` for `label`, `hint`, `value`, `tooltip`.

**Why**:
- Most accessibility labels are short (< 24 bytes)
- SmolStr inlines small strings, avoiding heap allocation
- O(1) clone for all sizes
- Used by xilem and slint for similar purposes

**Trade-offs**:
- Slight API difference (SmolStr vs String)
- Additional dependency

### Decision 3: Use SmallVec for collections

**What**: Replace `Vec` with `SmallVec` for `children` and `actions`.

**Why**:
- Most nodes have few children (< 4)
- Avoids heap allocation for common case
- Stack allocation is faster
- Used by xilem and egui

**Configuration**:
```rust
children: SmallVec<[SemanticsId; 4]>  // 4 children inline
actions: SmallVec<[SemanticsAction; 4]>  // 4 actions inline
```

### Decision 4: Unify ID types

**What**: Use `SemanticsId` (from flui-foundation) instead of `SemanticsNodeId`.

**Why**:
- Consistent with `ElementId`, `RenderId`, `LayerId`
- Uses NonZeroUsize for Option niche optimization
- Follows established +1/-1 slab index pattern

**Migration**:
```rust
// Old (flui_rendering)
SemanticsNodeId::from_index(5)  // Internal: 6

// New (flui-semantics)
SemanticsId::new(6)  // Same pattern as other IDs
```

### Decision 5: AccessKit in flui-platform, not flui-semantics

**What**: AccessKit integration belongs in `flui-platform`, not `flui-semantics`.

**Why**:
- `flui-semantics` = abstract semantic model (types, tree, configuration)
- `flui-platform` = platform integrations (windows, GPU, input, accessibility)
- AccessKit requires platform-specific code (Windows/macOS/Linux have different APIs)
- Follows Flutter pattern: semantics is abstract, embedder handles platform conversion

**Architecture**:
```
flui-semantics/           # Abstract model
  SemanticsNode
  SemanticsTree
  SemanticsNodeData       # Serialization format

flui-platform/            # Platform integration
  accessibility/
    accesskit_adapter.rs  # SemanticsTree → accesskit::TreeUpdate
    action_handler.rs     # accesskit::Action → SemanticsAction
```

**Benefits**:
- flui-semantics stays pure, no platform dependencies
- Platform-specific accessibility code is colocated with other platform code
- Easier to add alternative accessibility backends if needed

### Decision 6: Keep SemanticsConfiguration pattern

**What**: Retain `SemanticsConfiguration` as the builder for node properties.

**Why**:
- Flutter-compatible pattern
- Clean separation: Configuration (builder) vs Node (tree storage)
- Supports the "describe semantics" pattern from RenderObject

## Module Structure

```
flui-semantics/
├── src/
│   ├── lib.rs              # Crate root, re-exports
│   ├── action.rs           # SemanticsAction, ActionArgs, handlers
│   ├── configuration.rs    # SemanticsConfiguration
│   ├── event.rs            # SemanticsEvent, SemanticsEventType
│   ├── flags.rs            # SemanticsFlag, SemanticsFlags
│   ├── node.rs             # SemanticsNode, SemanticsNodeData
│   ├── owner.rs            # SemanticsOwner (tree management)
│   ├── properties.rs       # SemanticsProperties, AttributedString
│   ├── tree.rs             # SemanticsTree (slab storage)
│   ├── update.rs           # SemanticsUpdate, SemanticsUpdateBuilder
│   └── platform/           # Platform integrations (feature-gated)
│       ├── mod.rs
│       └── accesskit.rs    # AccessKit conversions
└── Cargo.toml
```

## Type Mappings

### FLUI → AccessKit

| FLUI Type | AccessKit Equivalent |
|-----------|---------------------|
| `SemanticsId` | `NodeId` |
| `SemanticsNode` | `Node` |
| `SemanticsConfiguration` | `NodeBuilder` |
| `SemanticsAction::Tap` | `Action::Click` |
| `SemanticsAction::LongPress` | `Action::Click` (custom) |
| `SemanticsFlag::IsButton` | `Role::Button` |
| `SemanticsFlag::IsTextField` | `Role::TextInput` |
| `label` | `Name` |
| `value` | `Value` |
| `hint` | `Description` |

### String Type Migration

| Field | Old Type | New Type |
|-------|----------|----------|
| `label` | `Option<String>` | `Option<SmolStr>` |
| `value` | `Option<String>` | `Option<SmolStr>` |
| `hint` | `Option<String>` | `Option<SmolStr>` |
| `tooltip` | `Option<String>` | `Option<SmolStr>` |
| `SemanticsTag::name` | `String` | `SmolStr` |

### Collection Type Migration

| Field | Old Type | New Type |
|-------|----------|----------|
| `children` | `Vec<SemanticsId>` | `SmallVec<[SemanticsId; 4]>` |
| `actions` | `Vec<SemanticsAction>` | `SmallVec<[SemanticsAction; 4]>` |
| `tags` | `HashSet<SemanticsTag>` | `FxHashSet<SemanticsTag>` |
| `custom_actions` | `Vec<CustomSemanticsAction>` | `SmallVec<[CustomSemanticsAction; 2]>` |

## Risks / Trade-offs

### Risk 1: Breaking API changes
**Mitigation**: Document migration path, provide deprecation warnings where possible.

### Risk 2: SmolStr API differences
**Mitigation**: SmolStr implements `AsRef<str>`, `Deref<Target=str>`, so most code works unchanged.

### Risk 3: AccessKit version compatibility
**Mitigation**: Pin to stable accesskit version (0.21), monitor for breaking changes.

## Implementation Status

### Completed (December 2024)

1. **Core Type Migration** ✅
   - All types migrated from `flui_rendering/src/semantics/` to `flui-semantics`
   - `SemanticsAction`, `SemanticsFlag`, `SemanticsConfiguration`, `SemanticsProperties`
   - `SemanticsEvent`, `SemanticsEventType`, `SemanticsEventData`
   - `SemanticsNode`, `SemanticsTree`, `SemanticsOwner`
   - `SemanticsNodeData`, `SemanticsUpdate`, `SemanticsUpdateBuilder`

2. **Optimizations Applied** ✅
   - `SmolStr` for label/hint/value/tooltip fields
   - `SmallVec<[SemanticsId; 4]>` for children
   - `SmallVec<[SemanticsAction; 4]>` for actions
   - `FxHashMap` for action handlers
   - `FxHashSet` for tags

3. **Integration** ✅
   - `flui_rendering` re-exports `flui_semantics as semantics`
   - `SemanticsNode.to_node_data()` creates proper `SemanticsNodeData`
   - All 409 workspace tests passing

4. **Cleanup** ✅
   - Removed `flui_rendering/src/semantics/` directory entirely
   - Removed `flui_types/src/semantics/` directory entirely
   - ~4400 lines of duplicate code removed

### Remaining Work (this proposal)

1. **Documentation** (12.2-12.3)
   - Usage examples
   - Migration guide

2. **Quality**
   - Run clippy on workspace

### Future Work (separate proposal)

**AccessKit Integration in flui-platform**
- Moved from this proposal to flui-platform scope
- Location: `flui-platform/src/accessibility/`
- Converts `SemanticsTree` → `accesskit::TreeUpdate`
- Handles platform-specific accessibility APIs

## Open Questions (Resolved)

1. ~~Should `flui_types/src/semantics/` be completely removed or kept for shared enums?~~
   - **Resolved**: Removed entirely. All types now in flui-semantics.

2. ~~Should we implement `From<SemanticsNode>` for `accesskit::Node` or require explicit conversion?~~
   - **Resolved**: Conversion will be in flui-platform, not flui-semantics. Explicit adapter pattern.

3. ~~Should `SemanticsOwner` manage the accesskit `TreeUpdate` directly?~~
   - **Resolved**: No. SemanticsOwner stays abstract. flui-platform handles TreeUpdate creation.
