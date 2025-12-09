# FLUI Architecture Documentation

This directory contains architecture documentation, design proposals, and implementation notes for the FLUI framework.

---

## 📖 Current Documentation

### Typestate System

- **[TYPESTATE_DESIGN.md](TYPESTATE_DESIGN.md)** ✅ **CURRENT** - Final typestate design
  - Structural vs Lifecycle separation
  - NavigableHandle extension trait
  - TreeInfo with usize (universal abstraction)
  - Usage examples for all three trees

### Archived / Outdated Typestate Docs

The following documents were created during exploration and are now **superseded** by `TYPESTATE_DESIGN.md`:

- ~~FULL_TYPESTATE_LIFECYCLE.md~~ - Described 4-state system (Unmounted/Mounted/Dirty/Reassembling)
  - **Status**: ⚠️ Outdated - simplified to 2-state system
- ~~TYPESTATE_PROPOSAL.md~~ - Initial exploration of typestate vs AnyView
  - **Status**: ⚠️ Outdated - decided on typestate with NavigableHandle
- ~~TYPESTATE_AS_TREE_ABSTRACTION.md~~ - Explored typestate as universal abstraction
  - **Status**: ⚠️ Partially outdated - see TYPESTATE_DESIGN.md for final approach
- ~~TYPESTATE_INTEGRATION_WITH_ARITY.md~~ - Integration with existing arity system
  - **Status**: ⚠️ Partially outdated - final design in TYPESTATE_DESIGN.md
- ~~TYPESTATE_API_EXAMPLES.md~~ - API examples
  - **Status**: ⚠️ Outdated - examples in TYPESTATE_DESIGN.md are current

### POC Code

- `poc_typestate.rs` - Proof of concept for typestate pattern
- `poc_anyview.rs` - Proof of concept for AnyView approach (not chosen)

---

## 📋 Design Decisions Summary

### Typestate System (Implemented)

**Final Decision**: 2-state structural typestate + runtime lifecycle flags

```rust
// Structural (typestate - compile-time)
Unmounted → Mounted

// Lifecycle (runtime flags)
needs_build, needs_layout, needs_paint
```

**Key Traits**:
- `Mountable` - Unmounted → Mounted transition
- `Unmountable` - Mounted → Unmounted transition + tree_info access
- `NavigableHandle` - Auto-implemented extension trait for navigation

**Implementation**: `crates/flui-tree/src/state.rs`

### TreeInfo Design

**Decision**: Use `usize` for universal abstraction, convert to typed IDs at domain boundary

```rust
// flui-tree (universal)
pub struct TreeInfo {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: usize,
}

// flui-view (domain-specific)
impl ViewHandle<Mounted> {
    pub fn parent(&self) -> Option<ViewId> {
        self.tree_info().parent.map(ViewId::from_raw)
    }
}
```

### RenderHandle Structure

**Decision**: Separate concerns in struct fields

```rust
pub struct RenderHandle<S: NodeState, P: Protocol> {
    tree_info: Option<TreeInfo>,              // Structural (who/where)
    state: Option<RenderState<P>>,            // Lifecycle + geometry
    parent_data: Option<Box<dyn Any>>,        // Hints from parent
}
```

---

## 🚀 Implementation Roadmap

### ✅ Completed

- [x] **Phase 1**: Implement typestate in flui-tree
  - [x] Unmounted and Mounted states
  - [x] Mountable/Unmountable traits
  - [x] NavigableHandle extension trait
  - [x] TreeInfo structure
  - [x] Full test coverage (121 tests)

### 🔄 In Progress

- [ ] **Phase 2**: Apply typestate to ViewHandle (flui-view)
- [ ] **Phase 3**: Apply typestate to ElementHandle (flui-element)
- [ ] **Phase 4**: Apply typestate to RenderHandle (flui_rendering)
- [ ] **Phase 5**: Implement Flutter-like child mounting API

---

## 📚 Reading Order

For newcomers to the FLUI typestate system:

1. **Start here**: [TYPESTATE_DESIGN.md](TYPESTATE_DESIGN.md)
   - Core concepts (structural vs lifecycle)
   - Trait design
   - Usage examples

2. **Implementation**: `crates/flui-tree/src/state.rs`
   - Actual code with comprehensive docs
   - 121 tests showing usage

3. **Context** (optional): Archived docs
   - Show the exploration process
   - Explain why certain approaches were rejected

---

## 🔗 Related Documentation

- **Main README**: [/README.md](../../README.md)
- **Build Guide**: [/BUILD.md](../../BUILD.md)
- **Claude Guide**: [/CLAUDE.md](../../CLAUDE.md)
- **Arity System**: [/crates/flui-tree/src/arity/mod.rs](../../crates/flui-tree/src/arity/mod.rs)

---

**Last Updated**: 2025-12-09
