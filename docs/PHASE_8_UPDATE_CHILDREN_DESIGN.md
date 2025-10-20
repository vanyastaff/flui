# Phase 8: Multi-Child Update Algorithm - Design Document

## 🎯 Goal

Implement Flutter's `updateChildren()` algorithm for efficient multi-child element updates.

---

## ❌ Current Problem

### Current Implementation (multi.rs:166-171)

```rust
fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)> {
    // ...
    let children = self.widget.children();
    children
        .iter()
        .enumerate()
        .map(|(slot, child_widget)| (self.id, dyn_clone::clone_box(child_widget.as_ref()), slot))
        .collect()
}
```

**Problems:**
1. ❌ **No element reuse** - Creates new elements every rebuild
2. ❌ **Ignores keys** - Doesn't handle keyed children
3. ❌ **Inefficient** - Unmounts all old children, mounts all new
4. ❌ **Loses state** - StatefulWidget state is destroyed
5. ❌ **No optimization** - Always processes all children

### Example Problem

```rust
// Frame 1: [A, B, C]
// Frame 2: [A, C, B]  // B and C swapped

// Current behavior:
// - Unmount A, B, C
// - Mount new A, new C, new B
// Result: All state lost, 6 operations

// Desired behavior:
// - Keep A (same position)
// - Move C to position 1
// - Move B to position 2
// Result: State preserved, 2 move operations
```

---

## ✅ Solution: Flutter's updateChildren() Algorithm

### High-Level Algorithm

```
Input:
  - old_children: Vec<ElementId>  // Current child elements
  - new_widgets: Vec<Box<dyn AnyWidget>>  // New child widgets

Output:
  - new_children: Vec<ElementId>  // Updated child elements

Steps:
  1. Build key maps (if any children have keys)
  2. Scan from start - update matching children in-place
  3. Scan from end - update matching children in-place
  4. Handle middle section:
     a. Remove old children not in new list
     b. Update/move keyed children
     c. Insert new children
```

### Detailed Algorithm

```rust
fn update_children(
    &mut self,
    old_children: Vec<ElementId>,
    new_widgets: Vec<Box<dyn AnyWidget>>,
) -> Vec<ElementId> {
    // Step 1: Build key → element map for old keyed children
    let old_keyed = build_key_map(&old_children);

    // Step 2: Build key → widget map for new keyed children
    let new_keyed = build_widget_key_map(&new_widgets);

    let mut new_children = Vec::with_capacity(new_widgets.len());
    let mut old_index = 0;
    let mut new_index = 0;

    // Phase 1: Scan from start, update in-place while elements match
    while old_index < old_children.len() && new_index < new_widgets.len() {
        let old_child = old_children[old_index];
        let new_widget = &new_widgets[new_index];

        if can_update(old_child, new_widget) {
            // Update in-place
            update_child(old_child, new_widget.clone(), new_index);
            new_children.push(old_child);
            old_index += 1;
            new_index += 1;
        } else {
            break;  // Mismatch, proceed to middle section
        }
    }

    // Phase 2: Scan from end, update in-place while elements match
    let mut old_end = old_children.len();
    let mut new_end = new_widgets.len();

    while old_index < old_end && new_index < new_end {
        let old_child = old_children[old_end - 1];
        let new_widget = &new_widgets[new_end - 1];

        if can_update(old_child, new_widget) {
            old_end -= 1;
            new_end -= 1;
        } else {
            break;  // Mismatch
        }
    }

    // Phase 3: Handle middle section (the complex part)
    if old_index < old_end || new_index < new_end {
        handle_middle_section(
            &old_children[old_index..old_end],
            &new_widgets[new_index..new_end],
            &old_keyed,
            &new_keyed,
            &mut new_children,
            new_index,
        );
    }

    // Phase 4: Update children from end scan
    for i in new_end..new_widgets.len() {
        let old_index = old_end + (i - new_end);
        let old_child = old_children[old_index];
        update_child(old_child, new_widgets[i].clone(), i);
        new_children.push(old_child);
    }

    new_children
}
```

---

## 🔑 Key Concepts

### 1. can_update() Logic

```rust
fn can_update(element: &dyn AnyElement, widget: &dyn AnyWidget) -> bool {
    // Same type AND (no keys OR same key)
    element.widget_type() == widget.type_name() &&
    match (element.key(), widget.key()) {
        (None, None) => true,
        (Some(k1), Some(k2)) => k1.equals(k2),
        _ => false,
    }
}
```

### 2. Keyed vs Unkeyed Children

**Unkeyed children:**
- Matched by position
- `can_update()` only checks type
- Fast path: O(n) scan

**Keyed children:**
- Matched by key
- Requires HashMap lookups
- Supports reordering
- Preserves state across moves

### 3. Slot Management

**Slot** = position in parent's child list

```rust
update_child(element_id, new_widget, slot_index);
//                                    ^^^^^^^^^^
//                                    Position in parent
```

---

## 📊 Example Walkthrough

### Example 1: Simple Append

```
Old: [A, B, C]
New: [A, B, C, D]

Phase 1 (scan start):
  - A matches A → update in-place, advance both
  - B matches B → update in-place, advance both
  - C matches C → update in-place, advance both
  - old_index=3, new_index=3

Phase 2 (scan end):
  - old_end=3, new_end=4, skip (old_index == old_end)

Phase 3 (middle):
  - No middle section (old_index == old_end)

Phase 4 (insert remaining):
  - Create element for D, slot=3

Result: [A, B, C, D]
Operations: 3 updates, 1 insert = 4 ops
```

### Example 2: Swap with Keys

```
Old: [A(key=1), B(key=2), C(key=3)]
New: [A(key=1), C(key=3), B(key=2)]  // B and C swapped

Phase 1 (scan start):
  - A matches A → update, advance both
  - B doesn't match C → break
  - old_index=1, new_index=1

Phase 2 (scan end):
  - old_end=3, new_end=3, skip check

Phase 3 (middle section):
  old_middle = [B(key=2), C(key=3)]
  new_middle = [C(key=3), B(key=2)]

  Build key maps:
    old_keyed = {2: B_element, 3: C_element}
    new_keyed = {3: C_widget, 2: B_widget}

  Process new_middle[0] = C(key=3):
    - Lookup key=3 in old_keyed → found C_element
    - Update C_element with C_widget
    - Add C_element to new_children, slot=1

  Process new_middle[1] = B(key=2):
    - Lookup key=2 in old_keyed → found B_element
    - Update B_element with B_widget
    - Add B_element to new_children, slot=2

Result: [A, C, B]
Operations: 3 updates (no unmount/remount!)
State preserved: ✅
```

### Example 3: Remove Middle

```
Old: [A, B, C, D, E]
New: [A, E]

Phase 1 (scan start):
  - A matches A → update, advance
  - B doesn't match E → break
  - old_index=1, new_index=1

Phase 2 (scan end):
  - old_children[4]=E, new_widgets[1]=E
  - E matches E → move pointers back
  - old_end=4, new_end=1

Phase 3 (middle):
  old_middle = [B, C, D]
  new_middle = []

  → Unmount B, C, D

Phase 4 (end section):
  - Update E with E_widget, slot=1

Result: [A, E]
Operations: 2 updates, 3 unmounts = 5 ops
```

---

## 🏗️ Implementation Plan

### Phase 8.1: Helper Functions

1. **Build key maps**
   ```rust
   fn build_key_map(children: &[ElementId]) -> HashMap<KeyId, ElementId>
   ```

2. **can_update() check**
   ```rust
   fn can_update(element: &dyn AnyElement, widget: &dyn AnyWidget) -> bool
   ```

3. **update_child() wrapper**
   ```rust
   fn update_child(
       element_id: ElementId,
       new_widget: Box<dyn AnyWidget>,
       slot: usize,
       tree: &mut ElementTree,
   ) -> ElementId
   ```

### Phase 8.2: Main Algorithm

1. **update_children() method**
   - Implement three-phase scan
   - Handle middle section
   - Return new child list

2. **handle_middle_section()**
   - Build keyed maps
   - Process keyed children first
   - Insert new unkeyed children
   - Unmount unused old children

### Phase 8.3: Integration

1. **Replace rebuild() in MultiChildRenderObjectElement**
   ```rust
   fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)> {
       let old_children = std::mem::take(&mut self.children);
       let new_widgets = self.widget.children();

       self.children = self.update_children(old_children, new_widgets);

       Vec::new()  // No longer return widgets to ElementTree
   }
   ```

2. **Add IndexedSlot (optional enhancement)**
   ```rust
   pub struct IndexedSlot {
       pub index: usize,
       pub previous_sibling: Option<ElementId>,
   }
   ```

### Phase 8.4: Testing

1. **Unit tests**
   - Empty list updates
   - Append/prepend
   - Remove from start/middle/end
   - Swap adjacent
   - Swap non-adjacent
   - Reorder completely

2. **Integration tests**
   - With StatefulWidget (verify state preservation)
   - With mixed keyed/unkeyed
   - Stress test (100+ children)

---

## 🎯 Success Criteria

✅ **Correctness:**
- All children updated correctly
- State preserved for moved keyed children
- Slots assigned correctly

✅ **Performance:**
- O(n) for common cases (append, prepend, simple changes)
- O(n log n) worst case (complete reorder)
- Minimize element creation/destruction

✅ **Compatibility:**
- Works with existing element types
- Maintains render tree consistency
- Proper lifecycle management

---

**Next:** Start implementation with helper functions!
