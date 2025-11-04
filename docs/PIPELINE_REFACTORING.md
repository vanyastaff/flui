# Pipeline Architecture Refactoring

**Status:** ✅ Complete
**Issue:** [#15](https://github.com/vanyastaff/flui/issues/15)
**Date:** November 2025

## Overview

The pipeline architecture was refactored to eliminate the "God Object" anti-pattern and follow SOLID principles. The monolithic `PipelineOwner` (1,323 LOC) was broken into focused components with clear responsibilities.

## Problem Statement

### Before Refactoring

`PipelineOwner` had **9+ responsibilities**:
1. Element tree management
2. Build pipeline coordination
3. Layout pipeline coordination
4. Paint pipeline coordination
5. Root element management
6. Dirty tracking
7. Frame scheduling
8. Error handling
9. Performance monitoring

This violated the **Single Responsibility Principle** and caused:
- 🔴 High coupling between unrelated subsystems
- 🔴 Difficult testing (hard to mock/isolate)
- 🔴 Complex maintenance (changes touched many concerns)
- 🔴 Poor extensibility (new features bloated the class)

## Solution

### After Refactoring

```rust
PipelineOwner (778 LOC - thin facade)
  ├─ tree: Arc<RwLock<ElementTree>>
  ├─ coordinator: FrameCoordinator (344 LOC)
  │   ├─ build: BuildPipeline
  │   ├─ layout: LayoutPipeline
  │   └─ paint: PaintPipeline
  ├─ root_mgr: RootManager (204 LOC)
  └─ Optional features:
      ├─ metrics: PipelineMetrics
      ├─ recovery: ErrorRecovery
      ├─ cancellation: CancellationToken
      └─ frame_buffer: TripleBuffer
```

### Components

#### 1. FrameCoordinator
**Location:** `crates/flui_core/src/pipeline/frame_coordinator.rs`
**Responsibility:** Orchestrate build→layout→paint phases
**Size:** 344 lines

```rust
pub struct FrameCoordinator {
    build: BuildPipeline,
    layout: LayoutPipeline,
    paint: PaintPipeline,
}
```

**Key Methods:**
- `build_frame()` - Orchestrates all three phases
- `flush_build()` - Executes build phase
- `flush_layout()` - Executes layout phase
- `flush_paint()` - Executes paint phase

#### 2. RootManager
**Location:** `crates/flui_core/src/pipeline/root_manager.rs`
**Responsibility:** Manage root element
**Size:** 204 lines

```rust
pub struct RootManager {
    root_id: Option<ElementId>,
}
```

**Key Methods:**
- `set_root()` - Set root element
- `root_id()` - Get current root ID
- `clear_root()` - Clear root element

#### 3. PipelineOwner (Refactored)
**Location:** `crates/flui_core/src/pipeline/pipeline_owner.rs`
**Responsibility:** Facade over focused components
**Size:** 778 lines (down from 1,323)

```rust
pub struct PipelineOwner {
    tree: Arc<RwLock<ElementTree>>,
    coordinator: FrameCoordinator,
    root_mgr: RootManager,
    // Optional features...
}
```

**Pattern:** Facade Pattern (Gang of Four)

## SOLID Principles Applied

### 1. Single Responsibility Principle ✅
Each component has ONE reason to change:
- `FrameCoordinator`: Phase orchestration logic changes
- `RootManager`: Root management logic changes
- `PipelineOwner`: API surface changes

### 2. Open/Closed Principle ✅
Easy to extend without modifying core:
- New pipeline phases → Add to `FrameCoordinator`
- New features → Add to `PipelineOwner` (minimal)
- Core components remain stable

### 3. Liskov Substitution Principle ✅
Components are independently testable:
- `FrameCoordinator` can be mocked
- `RootManager` can be tested in isolation
- No tight coupling

### 4. Interface Segregation Principle ✅
Focused interfaces:
- `FrameCoordinator` exposes only coordination methods
- `RootManager` exposes only root management
- No fat interfaces

### 5. Dependency Inversion Principle ✅
Depends on abstractions:
- Components work with `Arc<RwLock<ElementTree>>` (abstraction)
- Can swap implementations if needed

## Metrics

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| **Responsibilities** | 9+ | 1 (facade) | **-89%** |
| **LOC in PipelineOwner** | 1,323 | 778 | **-41%** |
| **Components** | 1 monolith | 3 focused | **+200%** |
| **Coupling** | High | Low | ✅ |
| **Testability** | Complex | Simple | ✅ |

## Benefits Achieved

### Maintainability ✅
- Changes localized to specific components
- Easier to understand (clear boundaries)
- Less cognitive overhead

### Testability ✅
- Each component independently testable
- Easy to mock/stub dependencies
- Faster test execution

### Extensibility ✅
- New features don't bloat `PipelineOwner`
- Clear places to add functionality
- Minimal API surface changes

### Performance 🚀
- Opens door for parallelization in `FrameCoordinator`
- Better cache locality (smaller structs)
- No performance regression

## Backward Compatibility

✅ **All public APIs preserved**
✅ **All tests pass** (245 tests migrated)
✅ **PipelineBuilder updated**
✅ **Zero breaking changes**

## Migration Guide

### For Users

No changes needed! The public API is identical:

```rust
// Old code (still works)
let mut owner = PipelineOwner::new();
owner.set_root(my_element);
let layer = owner.build_frame(constraints)?;

// New architecture (transparent)
// - Uses FrameCoordinator internally
// - Uses RootManager internally
// - Same API surface
```

### For Contributors

When adding new features:

1. **Pipeline phase changes** → Modify `FrameCoordinator`
2. **Root management changes** → Modify `RootManager`
3. **API changes** → Modify `PipelineOwner` facade
4. **Optional features** → Add to `PipelineOwner` with builder pattern

## Future Work

### Potential Improvements

1. **Parallelization** in `FrameCoordinator`
   - Build multiple subtrees in parallel
   - Layout independent branches concurrently

2. **Frame Scheduler** extraction (Phase 2 from issue)
   - Currently minimal logic in `PipelineMetrics`
   - Could extract if frame budgeting becomes complex

3. **Enhanced Testing**
   - Integration tests for `FrameCoordinator`
   - Property-based tests for invariants

## References

- **Issue:** [#15 - PipelineOwner is a God Object](https://github.com/vanyastaff/flui/issues/15)
- **Commit:** `ee2bf18` - refactor(pipeline): Break PipelineOwner God Object into focused components
- **Design Patterns:** Facade Pattern (GoF)
- **Principles:** SOLID (Robert C. Martin)

## Credits

Refactored with assistance from Claude Code.

---

**Last Updated:** November 2025
**Maintainer:** @vanyastaff
