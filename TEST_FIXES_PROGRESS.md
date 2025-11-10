# Test Fixes Progress for flui-core

## Summary

**Started with:** 237 test compilation errors
**Current status:** 145 test compilation errors
**Progress:** 92 errors fixed (-39%)

## Completed Fixes

### ✅ ElementId Comparison Issues (5 errors)
- Fixed all Children tests to use `ElementId::new()` instead of raw integers
- Updated comparison assertions to work with ElementId type

### ✅ Children Type Conversions (3 errors)
- Fixed `Children::Single()`, `Children::Multi()` constructors
- Updated all test assertions to use ElementId slices

### ✅ Method Argument Count (13 errors fixed)
- Fixed `effect.run_if_needed()` to pass `&mut ctx`
- Fixed `signal.get()` to pass `&mut ctx`
- Fixed `memo.get()` to pass `&mut ctx`

### ✅ Hook Tests Refactoring (10+ tests)
- Rewrote memo tests to use `Arc<Mutex<>>` pattern
- Rewrote effect tests with proper HookContext threading
- Pattern: `Arc::new(Mutex::new(0))` instead of `let mut count = 0`

### ✅ Test Infrastructure
- Added `testing/helpers.rs` with `test_hook_context()` utility
- Exported `TestWidget` for reusable test views
- Fixed testing module compilation

### ✅ Test Harness Hook Tests (10 errors fixed)
- Fixed `signal.get()` to pass `&mut ctx` or `harness.context_mut()`
- Fixed `memo.get()` to pass `&mut ctx`
- Updated memo tests to use Arc pattern for closures
- Pattern: `Arc::new(move |ctx: &mut HookContext| { ... })`

### ✅ Dependency Tests (36 errors fixed)
- Fixed all dependency.rs tests to use `ElementId::new()`
- Changed `let id = 1` to `let id = ElementId::new(1)`
- Fixed for loops: `for i in 1..=10` with `ElementId::new(i)` usage
- All DependencyInfo and DependencyTracker tests now compile

## Remaining Errors (145)

### Type Mismatches (133 errors) - E0308
Most common issues by file:
- `pipeline_owner.rs` (18) - Old test setup patterns
- `build_pipeline.rs` (18) - Pipeline API changes
- `foundation/error.rs` (17) - Error type conversions
- `pipeline/error.rs` (12) - Error handling changes
- `render/cache.rs` (11) - Cache API updates
- `foundation/notification.rs` (10) - Notification system changes
- `pipeline/parallel_build.rs` (9) - Parallel processing updates
- `pipeline/dirty_tracking.rs` (9) - Dirty tracking changes
- `pipeline/paint_pipeline.rs` (6) - Paint API updates
- `pipeline/layout_pipeline.rs` (6) - Layout API updates
- Smaller files: `recovery.rs` (4), `effect.rs` (3), `element_base.rs` (3), `diagnostics.rs` (3), `resource.rs` (2)

### Method Not Found (4 errors) - E0599
- Some methods renamed or removed in refactoring

### Moved Value Errors (2 errors) - E0382
- Some tests use values after move
- Need Arc cloning or restructuring

### Type Annotation Needed (4 errors) - E0282, E0283
- 2 errors: cannot infer type
- 2 errors: multiple implementations (need explicit type)

### Trait Bound Not Satisfied (1 error) - E0277
- Some type doesn't implement required trait

### Multiple Applicable Items (1 error) - E0034
- Ambiguous method call needs disambiguation

## Key API Changes Documented

### Hook Context Threading
```rust
// OLD
let signal = use_signal(0);
let value = signal.get();

// NEW
let mut ctx = test_hook_context();
let signal = ctx.use_hook::<SignalHook<_>>(0);
let value = signal.get(&mut ctx);
```

### Call Counting Pattern
```rust
// OLD (doesn't work with move closures)
let mut count = 0;
let closure = || { count += 1; };

// NEW (thread-safe)
let count = Arc::new(Mutex::new(0));
let count_clone = count.clone();
let closure = move || { *count_clone.lock() += 1; };
```

### ElementId Usage
```rust
// OLD
let children = vec![1, 2, 3];
assert_eq!(id, 42);

// NEW
let children = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
assert_eq!(id, ElementId::new(42));
```

## Next Steps

### Priority Order (by impact/difficulty ratio)

1. **Fix smaller test files first** (< 10 errors each)
   - `recovery.rs` (4 errors)
   - `effect.rs` (3 errors)
   - `element_base.rs` (3 errors)
   - `diagnostics.rs` (3 errors)
   - `resource.rs` (2 errors)
   - **Total: ~17 quick wins**

2. **Fix render cache tests** (11 errors)
   - Cache API updates
   - Likely similar patterns to fix

3. **Fix notification tests** (10 errors)
   - Notification system changes

4. **Fix pipeline dirty tracking** (9 errors)
   - Dirty tracking API updates

5. **Fix parallel build tests** (9 errors)
   - Parallel processing updates

6. **Fix pipeline tests** (6+6 = 12 errors)
   - `paint_pipeline.rs` (6)
   - `layout_pipeline.rs` (6)

7. **Fix error handling tests** (17+12 = 29 errors)
   - `foundation/error.rs` (17)
   - `pipeline/error.rs` (12)

8. **Fix large pipeline tests** (18+18 = 36 errors)
   - `build_pipeline.rs` (18)
   - `pipeline_owner.rs` (18)

9. **Fix edge cases** (8 errors)
   - Method not found (4)
   - Type annotations (4)
   - Moved values (2)
   - Trait bounds (1)
   - Ambiguous calls (1)

10. **Run test suite**
    - Once compilation passes
    - Fix any runtime failures
    - Ensure all tests pass

## Commands

```bash
# Check error count
cargo test -p flui_core --lib 2>&1 | grep "^error\[E" | wc -l

# Categorize errors
cargo test -p flui_core --lib 2>&1 | grep "^error\[E" | sort | uniq -c

# Test specific module
cargo test -p flui_core --lib hooks::

# Run all tests (when fixed)
cargo test -p flui_core
```
