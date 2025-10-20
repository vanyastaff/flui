# Phase 13: Performance Optimizations - Design Document

**Date:** 2025-10-20
**Status:** 🔄 In Progress

---

## Overview

Phase 13 implements performance optimizations inspired by Flutter's production-ready framework. Many optimizations are already complete from previous phases, so this phase focuses on the remaining gaps.

---

## What's Already Complete ✅

From **Phase 9** (RenderObject):
- ✅ Dirty-only layout (nodes_needing_layout)
- ✅ Dirty-only paint (nodes_needing_paint)
- ✅ Relayout boundaries (is_relayout_boundary API)
- ✅ Repaint boundaries (is_repaint_boundary API)

From **Phase 4** (BuildOwner):
- ✅ Dirty element sorting by depth (parents before children)
- ✅ Build scope tracking (in_build_scope flag)
- ✅ Deduplication (same element not scheduled twice)
- ✅ Build locking during finalize

---

## What Needs Implementation 🎯

### 13.1 Build Batching System

**Problem:** Multiple setState() calls in quick succession cause multiple rebuilds.

**Solution:** Batch multiple setState calls into single rebuild.

**Flutter Pattern:**
```dart
// Multiple setState calls
widget.setState(() => count++);
widget.setState(() => color = Colors.red);
widget.setState(() => text = "Updated");

// Result: Only 1 rebuild, not 3
```

**Rust Implementation:**
```rust
pub struct BuildBatcher {
    pending_builds: HashSet<ElementId>,
    batch_timer: Option<Instant>,
    batch_duration: Duration, // Default: 16ms (1 frame)
}

impl BuildOwner {
    /// Schedule build with batching
    pub fn schedule_build_batched(&mut self, element_id: ElementId, depth: usize) {
        // Add to batch
        // If batch timer expired, flush immediately
        // Otherwise wait for batch_duration
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        if let Some(start) = self.batcher.batch_timer {
            start.elapsed() >= self.batcher.batch_duration
        } else {
            false
        }
    }
}
```

**Benefits:**
- **3-10x fewer** builds for rapid state changes
- Smoother animations
- Less CPU usage

---

### 13.2 Inactive Element Pool

**Problem:** Creating/destroying elements is expensive (allocations, initialization).

**Solution:** Reuse deactivated elements instead of dropping them.

**Flutter Pattern:**
```dart
// Element is deactivated (removed from tree)
element.deactivate();

// Instead of drop(), move to inactive pool
inactiveElements.add(element);

// Later, reuse for same widget type
element = inactiveElements.findMatch(widget);
if (element != null) {
  element.activate(); // Reuse!
}
```

**Rust Implementation:**
```rust
pub struct InactiveElementPool {
    /// Pool of inactive elements by TypeId
    pool: HashMap<TypeId, Vec<Box<dyn AnyElement>>>,
    /// Max elements per type
    max_per_type: usize, // Default: 16
}

impl InactiveElementPool {
    /// Store inactive element
    pub fn store(&mut self, element: Box<dyn AnyElement>) {
        let type_id = element.widget_type_id();
        let pool = self.pool.entry(type_id).or_default();

        if pool.len() < self.max_per_type {
            pool.push(element);
        }
        // else drop (pool full)
    }

    /// Try to reuse element for widget
    pub fn take(&mut self, widget: &dyn AnyWidget) -> Option<Box<dyn AnyElement>> {
        let type_id = widget.type_id();
        self.pool.get_mut(&type_id)?.pop()
    }

    /// Clear pool (called periodically)
    pub fn clear(&mut self) {
        self.pool.clear();
    }
}

impl ElementTree {
    /// Unmount with pooling
    pub fn unmount_with_pool(&mut self, element_id: ElementId, pool: &mut InactiveElementPool) {
        if let Some(element) = self.remove(element_id) {
            // Deactivate
            element.deactivate();

            // Store in pool
            pool.store(element);
        }
    }
}
```

**Benefits:**
- **50-90% fewer** allocations for dynamic lists
- **2-5x faster** list updates (add/remove items)
- Lower memory fragmentation

**Trade-offs:**
- Small memory overhead (max 16 elements per type)
- Need periodic cleanup (every 5 seconds)

---

### 13.3 Smart Arc Cloning

**Problem:** Arc::clone() has overhead, even though it's cheap.

**Solution:** Minimize Arc clones by using references where possible.

**Current Pattern:**
```rust
// BuildContext clones tree Arc on every method call
pub fn parent(&self) -> Option<ElementId> {
    let tree = self.tree.read(); // Arc cloned here
    let element = tree.get(self.element_id)?;
    element.parent()
}
```

**Optimized Pattern:**
```rust
// Cache tree reference in Context
pub struct Context {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    // Add cached guard
    tree_guard_cache: RefCell<Option<RwLockReadGuard<'static, ElementTree>>>,
}

impl Context {
    /// Get tree with cached guard
    fn tree_cached(&self) -> &ElementTree {
        // Reuse guard if still valid
        // Otherwise create new one
    }
}
```

**Benefits:**
- **10-20% faster** Context method calls
- Fewer atomic operations

**Trade-offs:**
- More complex lifetime management
- Need unsafe for 'static trick

---

## Implementation Plan

### Step 1: Build Batching (High Priority)

1. Add `BuildBatcher` struct to BuildOwner
2. Add `schedule_build_batched()` method
3. Add `should_flush_batch()` + `flush_batch()` methods
4. Update StatefulElement to use batching
5. Add tests for batching behavior

**Files:**
- `src/tree/build_owner.rs` (+80 lines)

**Estimated Time:** 1 hour

---

### Step 2: Inactive Element Pool (Medium Priority)

1. Create `InactiveElementPool` struct
2. Add to ElementTree or BuildOwner
3. Update `unmount()` to use pool
4. Update `mount()` to try pool first
5. Add periodic cleanup (every 5 sec)
6. Add tests for pool behavior

**Files:**
- `src/tree/element_pool.rs` (new, ~200 lines)
- `src/tree/element_tree.rs` (+30 lines)

**Estimated Time:** 2 hours

---

### Step 3: Arc Optimization (Low Priority, Optional)

1. Profile Arc clone overhead
2. Add caching strategy if significant
3. Test safety and correctness
4. Benchmark improvement

**Files:**
- `src/context/mod.rs` (+50 lines)

**Estimated Time:** 1-2 hours (if pursued)

---

## Performance Impact Estimates

### Build Batching

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| 10 rapid setState() | 10 rebuilds | 1 rebuild | **10x** |
| Animation (60 fps) | 60 rebuilds/sec | 60 rebuilds/sec | 1x (same) |
| Bulk update (100 widgets) | 100 rebuilds | 1 rebuild | **100x** |

### Element Pooling

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| Add item to list (1000 items) | 5ms (alloc) | 0.5ms (reuse) | **10x** |
| Remove item | 2ms (drop) | 0.2ms (pool) | **10x** |
| Scroll ListView (recycle) | 10ms/frame | 2ms/frame | **5x** |

### Combined Impact

**Before Phase 13:**
- Rapid state changes: Slow (many rebuilds)
- Dynamic lists: Medium (allocations)
- Scrolling: Good (Phase 9 dirty tracking)

**After Phase 13:**
- Rapid state changes: **10x faster** (batching)
- Dynamic lists: **5-10x faster** (pooling)
- Scrolling: **Same** (already optimized)

**Overall:** 5-50x improvement for dynamic UIs with frequent updates!

---

## API Design

### Build Batching API

```rust
// Public API
impl BuildOwner {
    /// Enable build batching
    pub fn enable_batching(&mut self, duration: Duration) {
        self.batcher = Some(BuildBatcher::new(duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        self.batcher = None;
    }

    /// Schedule with automatic batching (if enabled)
    pub fn schedule_build(&mut self, element_id: ElementId, depth: usize) {
        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(element_id, depth);
        } else {
            self.schedule_build_for(element_id, depth);
        }
    }
}

// Usage
let mut owner = BuildOwner::new();
owner.enable_batching(Duration::from_millis(16)); // 1 frame

// Multiple setState calls
owner.schedule_build(id1, 0);
owner.schedule_build(id2, 1);
owner.schedule_build(id3, 2);

// Check if batch ready
if owner.should_flush_batch() {
    owner.flush_batch(); // Flushes all 3 at once
}
```

### Element Pool API

```rust
// Public API
impl ElementTree {
    /// Enable element pooling
    pub fn enable_pooling(&mut self, max_per_type: usize) {
        self.pool = Some(InactiveElementPool::new(max_per_type));
    }

    /// Disable element pooling
    pub fn disable_pooling(&mut self) {
        self.pool = None;
    }

    /// Get pool statistics
    pub fn pool_stats(&self) -> PoolStats {
        self.pool.as_ref().map(|p| p.stats()).unwrap_or_default()
    }
}

pub struct PoolStats {
    pub total_elements: usize,
    pub types_pooled: usize,
    pub reuse_count: usize,
    pub miss_count: usize,
}

// Usage
let mut tree = ElementTree::new();
tree.enable_pooling(16); // Max 16 per type

// Later check stats
let stats = tree.pool_stats();
println!("Reuse rate: {:.1}%",
    100.0 * stats.reuse_count as f64 / (stats.reuse_count + stats.miss_count) as f64
);
```

---

## Testing Strategy

### Build Batching Tests

```rust
#[test]
fn test_build_batching() {
    let mut owner = BuildOwner::new();
    owner.enable_batching(Duration::from_millis(16));

    let id1 = ElementId::new();
    let id2 = ElementId::new();

    owner.schedule_build(id1, 0);
    owner.schedule_build(id2, 1);

    // Should be batched
    assert!(!owner.should_flush_batch()); // Too soon

    // Wait for batch duration
    std::thread::sleep(Duration::from_millis(20));

    assert!(owner.should_flush_batch());
}
```

### Element Pool Tests

```rust
#[test]
fn test_element_pooling() {
    let mut tree = ElementTree::new();
    tree.enable_pooling(16);

    // Create and unmount element
    let element_id = tree.create_element(widget);
    tree.unmount_with_pool(element_id);

    // Try to reuse
    let stats_before = tree.pool_stats();
    let reused = tree.mount_or_reuse(new_widget);
    let stats_after = tree.pool_stats();

    assert_eq!(stats_after.reuse_count, stats_before.reuse_count + 1);
}
```

---

## Migration Strategy

### Backward Compatibility

All optimizations are **opt-in**:
- Build batching: Disabled by default
- Element pooling: Disabled by default
- Existing code continues to work unchanged

### Gradual Adoption

```rust
// Phase 1: No optimizations (current)
let mut owner = BuildOwner::new();

// Phase 2: Enable batching only
owner.enable_batching(Duration::from_millis(16));

// Phase 3: Enable pooling too
owner.tree().write().enable_pooling(16);

// Phase 4: Tune parameters
owner.enable_batching(Duration::from_millis(8)); // More aggressive
owner.tree().write().enable_pooling(32); // Larger pool
```

---

## Summary

**Priority:**
1. ✅ **High**: Build batching (biggest impact, easy to implement)
2. ✅ **Medium**: Element pooling (good impact, moderate complexity)
3. ⏸️ **Low**: Arc optimization (small impact, complex)

**Expected Results:**
- 5-50x faster for dynamic UIs
- Lower CPU usage
- Smoother animations
- Better battery life on mobile

**Completion Estimate:**
- Build batching: 1 hour
- Element pooling: 2 hours
- Documentation: 0.5 hour
- **Total: 3.5 hours**

---

**Next Steps:**
1. Implement BuildBatcher in build_owner.rs
2. Implement InactiveElementPool in new file
3. Add tests
4. Document usage
5. Mark Phase 13 complete!

