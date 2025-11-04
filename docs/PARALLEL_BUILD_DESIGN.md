# Parallel Build Design

## Current State Analysis

### Sequential Build Flow

```
rebuild_dirty() {
    1. Sort by depth (parent→child order)
    2. Deduplicate
    3. for each element:
       - Get element from tree
       - Rebuild element
       - Update tree
}
```

**Bottleneck**: Single-threaded loop processing all elements sequentially.

### Architecture Components

- **BuildPipeline**: Tracks dirty elements with depths `Vec<(ElementId, usize)>`
- **ElementTree**: Stores all elements, accessed via `&mut ElementTree`
- **Element types**: Component, Render, Provider

## Parallelization Strategy

### Key Insight: Independent Subtrees

```
        Root (depth 0)
       /    \
      A      B          ← Independent subtrees!
     / \    / \
    C   D  E   F
```

Subtrees A and B can rebuild in parallel IF they don't share state.

### Challenge: Shared Mutable Tree

Current code uses `&mut ElementTree` - exclusive access!

**Solution Options**:

1. **Fine-grained locking** (Arc<RwLock<ElementTree>> with per-element locks)
2. **Parallel batch processing** (group independent subtrees, process in parallel)
3. **Copy-on-write** (clone subtrees, merge results)

### Recommended Approach: Subtree Partitioning

```rust
// Phase 1: Identify root nodes of independent subtrees
let subtree_roots = partition_into_subtrees(&dirty_elements);

// Phase 2: Parallel rebuild per subtree
rayon::scope(|s| {
    for subtree in subtree_roots {
        s.spawn(|_| {
            rebuild_subtree_sequential(tree, subtree);
        });
    }
});
```

## Implementation Plan

### Phase 1: Subtree Partitioning Algorithm

```rust
/// Partition dirty elements into independent subtrees
///
/// Returns: Vec<Vec<(ElementId, usize)>>
/// Each inner Vec is a subtree that can be processed independently
fn partition_into_subtrees(dirty: &[(ElementId, usize)]) -> Vec<Vec<(ElementId, usize)>> {
    // Algorithm:
    // 1. Sort by depth
    // 2. For each element, check if it's descendant of any previous subtree root
    // 3. If not descendant, start new subtree
    // 4. If descendant, add to that subtree

    let mut subtrees: Vec<Vec<(ElementId, usize)>> = Vec::new();

    for &(element_id, depth) in dirty {
        let mut added = false;

        // Check if this element belongs to existing subtree
        for subtree in &mut subtrees {
            let root = subtree[0];
            if is_descendant(element_id, root.0) {
                subtree.push((element_id, depth));
                added = true;
                break;
            }
        }

        // Start new independent subtree
        if !added {
            subtrees.push(vec![(element_id, depth)]);
        }
    }

    subtrees
}
```

### Phase 2: Thread-Safe Tree Access

**Current**: `tree: &mut ElementTree`
**Needed**: `tree: &Arc<RwLock<ElementTree>>`

Already implemented in FrameCoordinator! Just need to use it.

```rust
// Current (FrameCoordinator)
let mut tree_guard = tree.write();
self.build.rebuild_dirty(&mut tree_guard);
```

Change to:

```rust
self.build.rebuild_dirty_parallel(tree, &subtrees);
```

### Phase 3: Parallel Execution with Rayon

```rust
pub fn rebuild_dirty_parallel(
    &mut self,
    tree: &Arc<RwLock<ElementTree>>,
    subtrees: &[Vec<(ElementId, usize)>]
) {
    use rayon::prelude::*;

    subtrees.par_iter().for_each(|subtree| {
        // Each thread gets read lock for checking + write lock for updates
        for &(element_id, depth) in subtree {
            let tree_guard = tree.write(); // Per-element write lock
            // Rebuild logic here
            drop(tree_guard);
        }
    });
}
```

## Performance Considerations

### Overhead Analysis

Parallel dispatch has overhead:
- Thread spawn/join
- Lock contention
- Cache coherency

**Break-even point**: ~50-100 elements per subtree

### Heuristic

```rust
const MIN_PARALLEL_ELEMENTS: usize = 50;

if dirty_count < MIN_PARALLEL_ELEMENTS {
    // Sequential rebuild (less overhead)
    rebuild_dirty_sequential(tree);
} else {
    // Parallel rebuild
    rebuild_dirty_parallel(tree, &subtrees);
}
```

## Testing Strategy

### Test Cases

1. **Correctness**: Parallel produces same result as sequential
2. **Performance**: Measure speedup with large trees
3. **Thread safety**: No data races under TSAN
4. **Edge cases**:
   - Single subtree (degenerates to sequential)
   - Empty tree
   - Many small subtrees vs few large subtrees

### Benchmark Setup

```rust
#[bench]
fn bench_sequential_build_1000_elements(b: &mut Bencher) {
    let tree = create_tree_1000_elements();
    b.iter(|| rebuild_sequential(&tree));
}

#[bench]
fn bench_parallel_build_1000_elements(b: &mut Bencher) {
    let tree = create_tree_1000_elements();
    b.iter(|| rebuild_parallel(&tree));
}
```

## Risks & Mitigation

### Risk 1: Lock Contention

**Problem**: Multiple threads fighting for `tree.write()`

**Mitigation**:
- Fine-grained locking (per-element)
- Lock-free data structures for read-heavy operations
- Batch writes

### Risk 2: Incorrect Subtree Partitioning

**Problem**: Incorrectly identifying independent subtrees

**Mitigation**:
- Extensive testing with tree walker
- Validation that subtrees don't overlap
- Fallback to sequential on conflict

### Risk 3: Regression for Small Trees

**Problem**: Parallel overhead > benefit for small trees

**Mitigation**:
- Adaptive threshold (MIN_PARALLEL_ELEMENTS)
- Benchmark-driven tuning
- Feature flag for opting out

## Feature Flag

```rust
#[cfg(feature = "parallel-build")]
pub fn rebuild_dirty(&mut self, tree: &Arc<RwLock<ElementTree>>) {
    if self.dirty_count() >= MIN_PARALLEL_ELEMENTS {
        self.rebuild_dirty_parallel(tree);
    } else {
        self.rebuild_dirty_sequential(tree);
    }
}
```

## Success Metrics

- [ ] 40%+ speedup for 1000+ element trees
- [ ] No regression for <100 element trees (<5% overhead)
- [ ] Linear scalability with cores (2-4 cores)
- [ ] All tests pass (including concurrent stress tests)

## Timeline

- **Day 1-2**: Implement subtree partitioning algorithm
- **Day 3**: Implement parallel rebuild with rayon
- **Day 4**: Benchmarking and tuning threshold
- **Day 5**: Testing, documentation, PR

## References

- [Flutter parallel build discussion](https://github.com/flutter/flutter/issues/14937)
- [React Fiber work-in-progress trees](https://github.com/acdlite/react-fiber-architecture)
- [Rayon parallel iterators](https://docs.rs/rayon/latest/rayon/)
