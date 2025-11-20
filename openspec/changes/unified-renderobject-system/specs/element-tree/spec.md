# Element Tree Specification Delta

## ADDED Requirements

### Requirement: RenderElement with Unified Type Erasure

The element tree SHALL provide a unified RenderElement type that stores protocol and arity as source of truth, with type-erased render objects via DynRenderObject trait.

#### Scenario: RenderElement created with protocol and arity information
- **WHEN** creating RenderElement::box_single(render_obj)
- **THEN** element stores protocol=Box, arity=Exact(1)
- **AND** element wraps render_obj in BoxRenderObjectWrapper<Single, R>
- **AND** protocol and arity are single source of truth (not duplicated in render_object)

#### Scenario: RenderElement constructors for each protocol/arity combination
- **WHEN** needing to create render elements
- **THEN** available constructors: box_leaf, box_optional, box_single, box_pair, box_variable
- **AND** available for sliver: sliver_single, sliver_variable
- **AND** each constructor enforces correct protocol at compile time

#### Scenario: RenderElement stores children in Vec<ElementId>
- **WHEN** adding children via push_child or replace_children
- **THEN** children stored in Vec for flexibility
- **AND** arity validation prevents invalid counts
- **AND** validation happens via debug_assert (zero cost in release)

#### Scenario: RenderElement provides typed children access via methods
- **WHEN** needing to inspect children
- **THEN** children() returns &[ElementId]
- **AND** runtime_arity() returns RuntimeArity enum
- **AND** protocol() returns LayoutProtocol
- **AND** All accessors are inline(always) for zero cost

### Requirement: Transactional Children Update API

The element tree SHALL provide a transactional API for safe batch children modifications that validates final state but allows intermediate violations.

#### Scenario: begin_children_update disables intermediate validation
- **WHEN** calling element.begin_children_update()
- **THEN** intermediate add/remove operations skip arity validation
- **AND** updating_children flag prevents panics from temporary violations
- **AND** all operations are queued for atomic commit

#### Scenario: remove_child operation safe during transaction
- **WHEN** calling element.remove_child(child_id) during transaction
- **THEN** child is removed from children vec
- **AND** no arity validation occurs (skipped during transaction)
- **AND** allows temporary violation of arity invariant
- **AND** marked pub(crate) to prevent misuse outside transactions

#### Scenario: commit_children_update validates final state
- **WHEN** calling element.commit_children_update()
- **THEN** updating_children flag is cleared
- **AND** arity validation runs on final children count
- **AND** panics if final state violates arity
- **AND** mark_needs_layout() called to schedule rebuild

#### Scenario: replace_children provides atomic alternative
- **WHEN** calling element.replace_children(new_children)
- **THEN** all children replaced atomically
- **AND** arity validation on new_children before replacement
- **AND** recommended for rebuilds and reconciliation
- **AND** no need for begin/commit for simple replacements

#### Scenario: arity violation messages include context
- **WHEN** arity violation occurs during push_child
- **THEN** panic message includes: render object name, expected arity, children.len()
- **AND** error message format: "[name] Arity violation: cannot add child, expected X children"
- **AND** helps debugging in large element trees

### Requirement: Render and Paint with DynGeometry

The element tree SHALL support layout and paint operations that work with both Box and Sliver protocols via DynGeometry and DynHitTestResult enums.

#### Scenario: layout() method accepts DynConstraints parameter
- **WHEN** calling element.layout(tree, DynConstraints::Box(constraints))
- **THEN** render object receives typed constraints
- **AND** returns DynGeometry::Box(size)
- **AND** DynGeometry stored in RenderState automatically

#### Scenario: layout() updates RenderState with computed geometry
- **WHEN** layout returns DynGeometry
- **THEN** RenderState::Box variant updated with size and constraints
- **AND** RenderState::Sliver variant updated with geometry and constraints
- **AND** needs_layout flag cleared via state.flags().clear_needs_layout()

#### Scenario: paint() method returns Canvas layer
- **WHEN** calling element.paint(tree, offset)
- **THEN** render object paint method called with typed offset
- **AND** returns Canvas (layer)
- **AND** needs_paint flag cleared automatically
- **AND** Canvas can be composed into parent layer

#### Scenario: hit_test() method checks point containment
- **WHEN** calling element.hit_test(tree, position, &mut result)
- **THEN** render object called with typed position
- **AND** returns DynHitTestResult (Box(bool) or Sliver(SliverHitTestResult))
- **AND** mutable result accumulates hit results from children

### Requirement: Centralized ElementTree Scheduling API

The element tree SHALL provide request_layout() and request_paint() methods that handle both dirty set marking and RenderState flag setting.

#### Scenario: request_layout marks element in dirty set and flags
- **WHEN** calling tree.request_layout(element_id)
- **THEN** element added to dirty_layout set
- **AND** RenderState.flags().mark_needs_layout() called
- **AND** both operations atomic (no intermediate state)
- **AND** tracing log includes element_id for debugging

#### Scenario: request_paint marks element in dirty set and flags
- **WHEN** calling tree.request_paint(element_id)
- **THEN** element added to dirty_paint set
- **AND** RenderState.flags().mark_needs_paint() called
- **AND** both operations atomic
- **AND** prevents the "marked but not flagged" bug

#### Scenario: Flag checks are lock-free via atomics
- **WHEN** coordinator checks render_state.needs_layout()
- **THEN** reads from AtomicRenderFlags without lock
- **AND** zero contention on heavily-read flags
- **AND** better scalability for parallel layout

#### Scenario: Flag updates via lock require read guard
- **WHEN** mark_needs_layout() is called
- **THEN** acquires read guard to access AtomicRenderFlags
- **AND** atomic flag update inside read guard
- **AND** prevents double-marking overhead
- **AND** write lock never held on flags (lock-free design)

### Requirement: Thread-Safe RenderElement with Explicit Lock Ordering

The element tree SHALL enforce strict lock ordering to prevent deadlocks: render_object lock acquired before render_state lock.

#### Scenario: Lock order documented and enforced
- **WHEN** needing both render_object and render_state
- **THEN** always acquire render_object lock first
- **AND** acquire render_state lock second
- **AND** never acquire in reverse order
- **AND** clear documentation in code comments and safety docs

#### Scenario: Layout method respects lock ordering
- **WHEN** element.layout() is called
- **THEN** acquires write lock on render_object first (for mutation)
- **AND** acquires write lock on render_state second (for state update)
- **AND** releases in reverse order (LIFO)
- **AND** no deadlock possible if all callers follow ordering

#### Scenario: Loom tests verify lock order correctness
- **WHEN** Loom deadlock prevention tests run
- **THEN** multiple threads access same element with correct lock order
- **AND** all threads complete without deadlock
- **AND** incorrect lock order is caught by Loom (test fails)
- **AND** part of CI validation suite

#### Scenario: Paint and debug_name respect lock ordering
- **WHEN** paint() reads render_object and render_state
- **THEN** acquires render_object read lock first
- **AND** may acquire render_state read lock second if needed
- **AND** all access patterns follow render_object → render_state order
- **AND** debug_name() in Debug impl acquires read lock safely

---

## MODIFIED Requirements

### Requirement: Element Enum Structure

The element tree element variants (Component, Render, Provider) SHALL be available with Render variant updated to use unified RenderElement.

#### Scenario: Element::Render variant stores unified RenderElement
- **WHEN** creating Element::Render(element)
- **THEN** element is RenderElement type
- **AND** RenderElement stores protocol, arity, and type-erased render_object
- **AND** no separate RenderNode enum (unified into RenderElement)

#### Scenario: Pattern matching on Element types
- **WHEN** matching Element enum
- **THEN** as_render() returns Option<&RenderElement>
- **AND** as_render_mut() returns Option<&mut RenderElement>
- **AND** is_render() returns bool
- **AND** parent() accessor works on all variants

#### Scenario: Element lifecycle unchanged
- **WHEN** managing element lifecycle
- **THEN** lifecycle() returns ElementLifecycle
- **AND** transitions Initial → Active → Inactive → Defunct unchanged
- **AND** parent() and other base methods work as before

### Requirement: ElementTree Child Layout and Paint Methods

The element tree SHALL provide helper methods for laying out and painting child elements correctly according to their protocol.

#### Scenario: layout_box_child returns Size
- **WHEN** render object calls ctx.layout_child(child_id, constraints)
- **THEN** child element is looked up in tree
- **AND** if child is Box protocol, dyn_layout called with DynConstraints::Box
- **AND** returns Size (unwrapped from DynGeometry::Box)
- **AND** panics if child is Sliver (protocol mismatch, shouldn't happen)

#### Scenario: layout_sliver_child returns SliverGeometry
- **WHEN** sliver render object calls ctx.layout_sliver_child(child_id, constraints)
- **THEN** child element is looked up in tree
- **AND** if child is Sliver protocol, dyn_layout called with DynConstraints::Sliver
- **AND** returns SliverGeometry (unwrapped from DynGeometry::Sliver)
- **AND** panics if child is Box (protocol mismatch)

#### Scenario: paint_box_child delegates to child element
- **WHEN** render object calls ctx.paint_child(child_id, offset)
- **THEN** child element.paint(tree, offset) called
- **AND** returns Canvas
- **AND** parent can compose child canvas into composite layer

#### Scenario: hit_test_box_child accumulates results
- **WHEN** render object calls ctx.hit_test_child(child_id, position)
- **THEN** child element.hit_test(tree, position, result) called
- **AND** result accumulated across all children
- **AND** returns bool indicating if child was hit

---

## REMOVED Requirements

### Requirement: Protocol Duplication in RenderNode

**Reason:** Protocol now stored only in RenderElement (single source of truth), not in RenderNode variants.

**Migration:**
- Remove protocol field from render object implementations
- Access protocol via element.protocol() instead
- Update any code that reads protocol from render object to read from RenderElement

### Requirement: Manual Lock Ordering Documentation

**Reason:** Now explicit in RenderElement implementation with inline comments and documented patterns.

**Migration:**
- No code changes needed for existing correct patterns
- Update any incorrect patterns that acquire render_state then render_object
- Add Loom tests to verify lock ordering
