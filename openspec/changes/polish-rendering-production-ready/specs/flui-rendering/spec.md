## ADDED Requirements

### Requirement: Structured Error Handling

The rendering system SHALL provide a `RenderError` enum for recoverable errors instead of panicking.

#### Scenario: Constraint violation during layout

- **GIVEN** a RenderBox receives constraints it cannot satisfy
- **WHEN** `perform_layout()` attempts to compute size
- **THEN** `RenderError::ConstraintViolation` SHALL be returned
- **AND** the error SHALL include the expected and actual constraint details

#### Scenario: Child layout failure propagation

- **GIVEN** a parent RenderBox laying out multiple children
- **WHEN** one child's `perform_layout()` returns an error
- **THEN** the error SHALL propagate to the parent as `RenderError::ChildLayoutFailed`
- **AND** the parent MAY choose to skip the failed child and continue

---

### Requirement: Pipeline Tracing Instrumentation

The rendering pipeline SHALL emit tracing spans for profiling and debugging.

#### Scenario: Layout phase instrumentation

- **GIVEN** `PipelineOwner::flush_layout()` is called
- **WHEN** dirty nodes are processed
- **THEN** a `layout` tracing span SHALL encompass the entire phase
- **AND** each node's layout SHALL be recorded with its duration
- **AND** the span SHALL include `dirty_count` as an attribute

#### Scenario: Paint phase instrumentation

- **GIVEN** `PipelineOwner::flush_paint()` is called
- **WHEN** dirty nodes are repainted
- **THEN** a `paint` tracing span SHALL encompass the entire phase
- **AND** repaint boundary hits/misses SHALL be logged at DEBUG level

---

### Requirement: Convenience Constructors

Common RenderObject configurations SHALL have ergonomic one-liner constructors.

#### Scenario: Flex row/column shortcuts

- **GIVEN** a developer needs a horizontal layout
- **WHEN** `RenderFlex::row()` is called
- **THEN** a RenderFlex configured with `Axis::Horizontal` SHALL be returned
- **AND** `RenderFlex::column()` SHALL return `Axis::Vertical` configuration

#### Scenario: Padding shortcuts

- **GIVEN** a developer needs uniform 8px padding
- **WHEN** `RenderPadding::all(8.0)` is called
- **THEN** EdgeInsets with equal values on all sides SHALL be created
- **AND** `RenderPadding::symmetric(h, v)` SHALL create horizontal/vertical padding

---

### Requirement: Builder Pattern for Complex Objects

RenderObjects with many configuration options SHALL provide a builder API.

#### Scenario: Stack builder configuration

- **GIVEN** a developer needs a customized RenderStack
- **WHEN** `RenderStack::builder()` is called
- **THEN** a builder object with chainable methods SHALL be returned
- **AND** `.alignment()`, `.clip_behavior()`, `.fit()` SHALL be available
- **AND** `.build()` SHALL return the configured RenderStack

---

### Requirement: Performance Benchmarks

The rendering system SHALL include criterion benchmarks validating performance.

#### Scenario: Layout throughput benchmark

- **GIVEN** a flex layout with 100 children
- **WHEN** the `bench_layout_flex_100_children` benchmark runs
- **THEN** layout throughput SHALL be measured in operations/second
- **AND** results SHALL be reproducible across runs

#### Scenario: Deep tree benchmark

- **GIVEN** a render tree with 100 levels of nesting
- **WHEN** the `bench_layout_deep_tree_100_levels` benchmark runs
- **THEN** constraint propagation time SHALL be measured
- **AND** the benchmark SHALL validate O(n) complexity

---

## MODIFIED Requirements

### Requirement: PaintingContext API

The rendering system SHALL provide a `PaintingContext` struct that follows Flutter's `PaintingContext` API for managing canvas painting and layer composition.

#### Scenario: Paint child at offset

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `paint_child(child_id, offset)` is called
- **THEN** the child's `paint()` method SHALL be invoked with the offset applied
- **AND** if the child is a repaint boundary, compositing SHALL be handled automatically
- **AND** the canvas reference MAY change after the call due to layer composition

#### Scenario: Push opacity layer

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `push_opacity(alpha, offset, painter_fn)` is called
- **THEN** an `OpacityLayer` SHALL be created if compositing is needed
- **AND** the painter function SHALL be called with a child context
- **AND** the layer SHALL apply the specified alpha value (0-255)

#### Scenario: Push clip rect layer

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `push_clip_rect(rect, clip_behavior, painter_fn)` is called
- **THEN** a `ClipRectLayer` SHALL be created if compositing is needed
- **OR** canvas clipping SHALL be used if no compositing is required
- **AND** `ClipBehavior::None` SHALL skip clipping entirely

#### Scenario: Push transform layer

- **GIVEN** a `PaintingContext` with an active canvas
- **WHEN** `push_transform(matrix, offset, painter_fn)` is called
- **THEN** a `TransformLayer` SHALL be created if compositing is needed
- **AND** the transform SHALL be applied relative to the offset

---

### Requirement: Flutter-Style Layout Protocol

The `RenderBox` and `RenderSliver` traits SHALL follow Flutter's layout protocol where constraints are passed as parameters to `perform_layout()`.

#### Scenario: Box perform_layout receives constraints

- **GIVEN** a `RenderBox<A>` implementation
- **WHEN** layout is triggered by the framework
- **THEN** `perform_layout(constraints: BoxConstraints)` SHALL be called
- **AND** the method SHALL return a `Size` that satisfies the constraints

#### Scenario: Intrinsic dimension computation

- **GIVEN** a `RenderBox<A>` implementation
- **WHEN** `compute_min_intrinsic_width(height)` is called
- **THEN** the minimum width to display content at that height SHALL be returned
- **AND** results SHOULD be cached to avoid repeated computation

#### Scenario: Dry layout for sizing without side effects

- **GIVEN** a `RenderBox<A>` implementation
- **WHEN** `compute_dry_layout(constraints)` is called
- **THEN** the size that would result from layout SHALL be returned
- **AND** the render object's state SHALL NOT be modified
- **AND** children SHALL NOT be laid out (use their cached dry layout)
