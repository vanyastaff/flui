# Bug: SizedBox Expands to Max Constraints When Dimension is None (FIXED)

## Problem

**Root Cause:** The bug was in `RenderSizedBox`, not in `RenderFlex`.

When `SizedBox::builder().height(4.0).build()` (width=None) is used, RenderSizedBox was taking `constraints.max_width` for the unspecified dimension instead of using the child's intrinsic size. This caused Columns containing SizedBox spacers to expand to full width (352px) instead of shrinking to content (~42-75px).

This manifested as Row overflow when:
1. Row uses `MainAxisAlignment::SpaceEvenly`
2. Each Column child contains `SizedBox` with only one dimension specified
3. SizedBox expands to max width, causing Columns to be too wide
4. Total width of 3 Columns exceeds Row width (1056px > 352px)

## Reproduction

### Test Case 1: Nested Columns in Row
```rust
// Container with width=400px
Container::builder()
    .width(400.0)
    .child(
        Row::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .child(Column::builder() /* ... */ .build()) // Child 1
            .child(Column::builder() /* ... */ .build()) // Child 2
            .child(Column::builder() /* ... */ .build()) // Child 3 OVERFLOWS!
            .build()
    )
    .build()
```

**Result:** Third Column renders outside the Container's bounds (400px).

### Visual Evidence

See examples:
- `examples/test_nested_columns_in_row.rs` - Shows only 2 of 3 columns
- `examples/test_column_width.rs` - Shows 3rd column (blue) overflowing container

**Screenshot:** Third column (blue background, "312 Following") renders beyond white container boundary.

## Root Cause

In `crates/flui_rendering/src/objects/layout/sized_box.rs` lines 91-106 (original code):

```rust
fn layout(&mut self, ctx: &LayoutContext) -> Size {
    let tree = ctx.tree;
    let child_id = ctx.children.single();
    let constraints = ctx.constraints;

    // PROBLEM: When width/height is None, takes max constraint instead of child size!
    let width = self.width.unwrap_or(constraints.max_width);  // ← BUG
    let height = self.height.unwrap_or(constraints.max_height);

    let size = Size::new(width, height);
    let child_constraints = BoxConstraints::tight(size);
    tree.layout_child(child_id, child_constraints);
    size
}
```

**Issue:** When `SizedBox::builder().height(4.0).build()` is used (width=None), the SizedBox takes `constraints.max_width` (352px) instead of the child's intrinsic width.

## Expected Behavior

According to Flutter's SizedBox behavior:
- If dimension (width/height) is specified, use that value
- If dimension is None, pass loose constraints and use **child's intrinsic size**
- Never expand to max constraint unless explicitly requested

## Flutter Comparison

Flutter's RenderConstrainedBox (which SizedBox uses) correctly handles this:
- Lays out child with loose constraints when dimension is None
- Returns child's actual size for unspecified dimensions

## Fix Applied

Fixed in `crates/flui_rendering/src/objects/layout/sized_box.rs`:

```rust
fn layout(&mut self, ctx: &LayoutContext) -> Size {
    let tree = ctx.tree;
    let child_id = ctx.children.single();
    let constraints = ctx.constraints;

    // Layout child first if we need its size
    let child_size = if self.width.is_none() || self.height.is_none() {
        // Need child's intrinsic size for unspecified dimensions
        // Give child loose constraints for dimensions we don't control
        let child_constraints = BoxConstraints::new(
            0.0,
            self.width.unwrap_or(constraints.max_width),
            0.0,
            self.height.unwrap_or(constraints.max_height),
        );
        tree.layout_child(child_id, child_constraints)
    } else {
        // Both dimensions specified, we don't need child size yet
        Size::ZERO
    };

    // Calculate final size - use child size for unspecified dimensions
    let width = self.width.unwrap_or(child_size.width);
    let height = self.height.unwrap_or(child_size.height);
    let size = Size::new(width, height);

    // If we already laid out child with correct constraints, we're done
    // Otherwise, force child to match our size
    if self.width.is_some() && self.height.is_some() {
        let child_constraints = BoxConstraints::tight(size);
        tree.layout_child(child_id, child_constraints);
    }

    size
}
```

## Impact

**Severity:** High (now FIXED)
- Affected all widgets using SizedBox with partial dimensions
- Caused visual layout corruption (content overflowing bounds)
- Broke common UI patterns (e.g., profile cards with Column spacing)

**Fix Date:** 2025-11-10

## Test Cases - Status After Fix

- ✅ `examples/test_nested_columns_in_row.rs` - All 3 columns now display correctly
- ✅ `examples/test_column_width.rs` - No overflow, all columns visible
- ✅ `examples/profile_card.rs` - Stats row displays correctly
- ✅ `examples/test_simple_row.rs` - Still works (simple Text widgets)
- ✅ `examples/hello_world_view.rs` - Still works (no Row)
- ✅ `examples/test_column_row.rs` - Still works

## Results

**Before Fix:**
```
Column 1: width=352px (should be ~42px)
Column 2: width=352px (should be ~75px)
Column 3: width=352px (should be ~75px)
Row: total_width=1056px, overflow=704px ❌
```

**After Fix:**
```
Column 1: width=42px ✅
Column 2: width=75.6px ✅
Column 3: width=75.6px ✅
Row: total_width=193.2px, no overflow ✅
```

## Related Files

- `crates/flui_rendering/src/objects/layout/flex.rs` - Main layout logic
- `crates/flui_types/src/layout/alignment.rs` - SpaceEvenly calculation
- `crates/flui_widgets/src/layout/row.rs` - Row widget
- `crates/flui_widgets/src/layout/column.rs` - Column widget
