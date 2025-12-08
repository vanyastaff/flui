# Proposal: Refactor flui-tree to Pure Tree Abstractions

## Summary

Transform `flui-tree` from a monolithic crate with domain-specific logic into a minimal, generic tree abstraction library that serves as the foundation for all FLUI tree types (View, Element, Render, Layer, Semantics).

## Motivation

Currently `flui-tree` contains ~18,600 lines of code mixing:
- Generic tree operations (navigation, iteration, visitors)
- Render-specific logic (dirty tracking, render iterators, render traits)
- View-specific logic (snapshots, diffs)
- Element-specific logic (lifecycle, reconciliation, inherited data)
- Pipeline-specific logic (phases, coordination)

This violates separation of concerns and makes it harder to:
1. Understand what the crate provides
2. Add new tree types (Layer, Semantics)
3. Maintain and test individual components

### Flutter Reference

Flutter has distinct tree implementations:
- **Widget Tree** - immutable, declarative descriptions
- **Element Tree** - mutable, manages lifecycle
- **RenderObject Tree** - layout and painting
- **Layer Tree** - compositing layers
- **Semantics Tree** - accessibility

Each tree shares common patterns (parent-child, traversal) but has domain-specific behavior.

## Proposed Solution

### What flui-tree Should Contain (Core Abstractions)

**~3,000 lines target** - minimal generic tree infrastructure:

```
flui-tree/
├── lib.rs
├── error.rs           # TreeError, TreeResult
├── traits/
│   ├── mod.rs
│   ├── node.rs        # TreeNode trait (NEW - simplified)
│   ├── read.rs        # TreeRead (simplified - no render logic)
│   ├── write.rs       # TreeWrite (simplified)
│   └── nav.rs         # TreeNav (simplified - no render logic)
├── iter/
│   ├── mod.rs
│   ├── ancestors.rs   # Generic ancestor iteration
│   ├── descendants.rs # Generic descendant iteration
│   ├── siblings.rs    # Generic sibling iteration
│   ├── breadth_first.rs
│   └── depth_first.rs
├── visitor/
│   ├── mod.rs         # TreeVisitor trait (simplified)
│   └── basic.rs       # CollectVisitor, CountVisitor, FindVisitor
└── arity/             # Keep - generic child count validation
    ├── mod.rs
    └── accessors.rs
```

### What Moves Where

| Current Location | Target Crate | Reason |
|-----------------|--------------|--------|
| `traits/render.rs` (1414 lines) | `flui_rendering` | Render-specific |
| `traits/dirty.rs` (1244 lines) | `flui_rendering` | Render-specific dirty tracking |
| `iter/render.rs` (1335 lines) | `flui_rendering` | Render-specific iterators |
| `iter/render_collector.rs` (995 lines) | `flui_rendering` | Render-specific |
| `traits/view.rs` (675 lines) | `flui-view` | View-specific (snapshots, diffs) |
| `traits/lifecycle.rs` (310 lines) | `flui-element` | Element lifecycle |
| `traits/reconciliation.rs` (427 lines) | `flui-element` | Element reconciliation |
| `traits/inherited.rs` (435 lines) | `flui-element` | Inherited data propagation |
| `traits/diff.rs` (629 lines) | `flui-element` | Tree diffing for reconciliation |
| `traits/pipeline.rs` (631 lines) | `flui-pipeline` | Pipeline coordination |
| `traits/context.rs` (328 lines) | `flui_core` | Framework context |
| `traits/validation.rs` (745 lines) | `flui_devtools` | Debug/validation tools |
| `traits/combined.rs` (351 lines) | DELETE | Convenience aliases, not needed |
| `visitor/statistics.rs` (490 lines) | `flui_devtools` | Debug statistics |

### Future Tree Crates

This refactoring enables clean creation of:
- `flui-layer` - Layer tree for compositing (will use flui-tree traits)
- `flui-semantics` - Semantics tree for accessibility (will use flui-tree traits)

## Impact

### Breaking Changes
- All crates depending on moved traits need import updates
- Some trait signatures may simplify (remove render-specific GATs)

### Benefits
- Clear separation of concerns
- Smaller, focused crates
- Easier to add new tree types
- Better testability
- Reduced compile times for crates that don't need all features

## Success Criteria

1. `flui-tree` contains only generic tree abstractions (~3,000 lines)
2. All moved code compiles in target crates
3. All existing tests pass
4. No circular dependencies
5. Clear documentation of what each crate provides
