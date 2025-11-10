# Test Fixes Progress for flui-core

## Summary

**Started with:** 237 test compilation errors
**Current status:** 194 test compilation errors
**Progress:** 43 errors fixed (-18%)

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

## Remaining Errors (194)

### Type Mismatches (165 errors)
Most common issues:
- Old API signatures vs new View API
- ElementId vs usize conversions
- Missing type annotations in closures
- BuildContext method signatures

### Missing Arguments (10 errors)
Methods still needing HookContext:
- Various hook methods in remaining test files
- Some signal/memo operations in integration tests

### Incorrect Arguments (7 errors)
Functions with changed signatures:
- BuildPipeline methods
- Element constructors
- Context creation

### ParentData Downcasting (2 errors)
- Need to implement proper downcast methods for ParentData trait
- Currently `Box<dyn ParentData>` doesn't have `downcast_ref()`

### Moved Value Errors (2 errors)
- Some tests use values after move
- Need Arc cloning or restructuring

### Miscellaneous (8 errors)
- 1 mark_dirty method not found
- 1 AnyView trait bound
- 2 type annotation errors
- 2 type annotation inference errors
- 1 multiple applicable items

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

1. **Fix remaining argument errors** (10 files)
   - Update signal/memo tests in remaining hook files
   - Add HookContext parameters

2. **Fix type mismatches** (priority by file)
   - `element/` tests - Element constructors
   - `pipeline/` tests - BuildPipeline API
   - `render/` tests - Render trait changes

3. **Fix ParentData downcasting**
   - Implement `downcast_ref()` for `Box<dyn ParentData>`
   - Or change API to not require downcasting

4. **Fix moved value errors**
   - Refactor tests to clone before move
   - Use Arc where needed

5. **Run test suite**
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
