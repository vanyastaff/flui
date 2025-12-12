# flui-rendering Spec Delta

## ADDED Requirements

### Requirement: PaintingContext API

The rendering system SHALL provide a `PaintingContext` struct that follows Flutter's `PaintingContext` API for managing canvas painting and layer composition.

#### Scenario: Paint child at offset

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `paint_child(child_id, offset)` is called
- **THEN** the child's `paint()` method SHALL be invoked with the offset applied
- **AND** the canvas reference MAY change after the call due to layer composition

#### Scenario: Push opacity layer

- **GIVEN** a `PaintingContext` with an active canvas  
- **WHEN** `push_opacity(alpha, painter_fn)` is called
- **THEN** an opacity layer SHALL be created if compositing is needed
- **AND** the painter function SHALL be called with a child context
- **AND** the layer SHALL apply the specified alpha value

#### Scenario: Push clip rect

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `push_clip_rect(rect, painter_fn)` is called  
- **THEN** a clip layer SHALL be created if compositing is needed
- **OR** the clip SHALL be applied directly to canvas if no compositing needed
- **AND** the painter function SHALL be called within the clipped region

#### Scenario: Push transform

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `push_transform(matrix, painter_fn)` is called
- **THEN** a transform layer SHALL be created if compositing is needed
- **AND** the painter function SHALL be called with transformed coordinates

---

### Requirement: Flutter-Style Layout Protocol

The `RenderBox` and `RenderSliver` traits SHALL follow Flutter's layout protocol where constraints are passed as parameters to `perform_layout()`, not via a context object.

#### Scenario: Box perform_layout receives constraints

- **GIVEN** a `RenderBox<A>` implementation
- **WHEN** layout is triggered by the framework
- **THEN** `perform_layout(constraints: BoxConstraints)` SHALL be called
- **AND** the method SHALL return a `Size` that satisfies the constraints
- **AND** no `LayoutContext` wrapper SHALL be used

#### Scenario: Sliver perform_layout receives constraints

- **GIVEN** a `RenderSliver<A>` implementation  
- **WHEN** layout is triggered by the framework
- **THEN** `perform_layout(constraints: SliverConstraints)` SHALL be called
- **AND** the method SHALL return a `SliverGeometry`
- **AND** no `LayoutContext` wrapper SHALL be used

#### Scenario: Child layout during perform_layout

- **GIVEN** a parent RenderBox with children
- **WHEN** `perform_layout()` needs to layout children
- **THEN** child layout SHALL be invoked via tree methods or stored child references
- **AND** the `parent_uses_size` flag SHALL be passed to determine relayout boundaries

---

### Requirement: Flutter-Style Hit Test Protocol

The `RenderBox` and `RenderSliver` traits SHALL follow Flutter's hit test protocol where `HitTestResult` is passed directly, not via a context wrapper.

#### Scenario: Box hit_test with result

- **GIVEN** a `RenderBox<A>` implementation
- **WHEN** hit testing is performed at a position
- **THEN** `hit_test(result: &mut BoxHitTestResult, position: Offset)` SHALL be called
- **AND** no `HitTestContext` wrapper SHALL be used
- **AND** the result SHALL accumulate hit entries with transform tracking

#### Scenario: Hit test children in reverse z-order

- **GIVEN** a RenderBox with multiple children
- **WHEN** `hit_test()` tests children
- **THEN** children SHALL be tested in reverse z-order (front to back)
- **AND** testing SHALL stop on first hit (short-circuit)
- **AND** `result.add_with_paint_offset()` SHALL be used for offset transformation

---

## MODIFIED Requirements

### Requirement: Canvas API Usage Patterns

RenderObjects in flui_rendering SHALL use the modern Canvas API patterns from flui_painting for improved readability, safety, and performance.

**Additionally**, RenderObjects SHALL receive canvas access via `PaintingContext` rather than direct canvas references, enabling proper layer management.

#### Scenario: Chaining API for transforms

**GIVEN** a RenderObject using manual save()/restore() pattern
**WHEN** the transform is simple (translate, rotate, scale)
**THEN** code SHALL use chaining API with saved()/restored()
**AND** transforms SHALL use translated(), rotated(), scaled_xy() methods
**AND** code SHALL be more concise and readable

#### Scenario: Consistent API usage across branches

**GIVEN** a RenderObject with conditional transforms (multiple branches)
**WHEN** some branches use chaining and others use old API
**THEN** all branches SHALL use consistent API (chaining)
**AND** saved() SHALL be used in all transform branches
**AND** restored() SHALL be called once at the end

#### Scenario: Canvas access via PaintingContext

**GIVEN** a RenderObject's `paint()` method
**WHEN** canvas operations are needed
**THEN** canvas SHALL be accessed via `ctx.canvas()` from PaintingContext
**AND** direct canvas parameters SHALL NOT be passed to paint()

---
