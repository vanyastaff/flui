# Rendering: Viewport and Sliver Protocol Specification

## ADDED Requirements

### Requirement: SliverConstraints User Scroll Direction

The `SliverConstraints` type SHALL include a `user_scroll_direction` field indicating the direction the user is attempting to scroll.

#### Scenario: User scrolling down in vertical viewport
- **WHEN** user scrolls down in a vertical viewport
- **THEN** `user_scroll_direction` SHALL be `ScrollDirection::Forward`

#### Scenario: User scrolling up in vertical viewport
- **WHEN** user scrolls up in a vertical viewport
- **THEN** `user_scroll_direction` SHALL be `ScrollDirection::Reverse`

#### Scenario: User not scrolling
- **WHEN** user is not actively scrolling
- **THEN** `user_scroll_direction` SHALL be `ScrollDirection::Idle`

### Requirement: SliverConstraints Preceding Scroll Extent

The `SliverConstraints` type SHALL include a `preceding_scroll_extent` field representing the total scroll extent consumed by all slivers that came before this sliver.

#### Scenario: First sliver in viewport
- **WHEN** a sliver is the first child of a viewport
- **THEN** `preceding_scroll_extent` SHALL be `0.0`

#### Scenario: Second sliver after 500px sliver
- **WHEN** a sliver follows a sliver with `scroll_extent = 500.0`
- **THEN** `preceding_scroll_extent` SHALL be `500.0`

### Requirement: GrowthDirection Enum

The system SHALL provide a `GrowthDirection` enum with variants `Forward` and `Reverse` to indicate the direction slivers grow relative to the axis direction.

#### Scenario: Forward growth in top-to-bottom axis
- **WHEN** `axis_direction` is `TopToBottom` and `growth_direction` is `Forward`
- **THEN** slivers SHALL be laid out from top to bottom

#### Scenario: Reverse growth in top-to-bottom axis
- **WHEN** `axis_direction` is `TopToBottom` and `growth_direction` is `Reverse`
- **THEN** slivers SHALL be laid out from bottom to top

### Requirement: RenderViewport Implements RenderBox

`RenderViewport` SHALL implement the `RenderBox<Variable>` trait to participate in the standard render tree layout and paint protocols.

#### Scenario: Viewport receives box constraints
- **WHEN** `RenderViewport::perform_layout()` is called with `BoxConstraints`
- **THEN** the viewport SHALL compute its size based on constraints
- **AND** the viewport SHALL layout all sliver children with `SliverConstraints`

#### Scenario: Viewport paints children
- **WHEN** `RenderViewport::paint()` is called
- **THEN** the viewport SHALL paint all visible sliver children
- **AND** the viewport SHALL clip content according to `clip_behavior`

### Requirement: Bidirectional Scrolling with Center Sliver

`RenderViewport` SHALL support bidirectional scrolling by allowing a `center` sliver to be designated as the zero scroll offset position.

#### Scenario: Center sliver at scroll offset zero
- **WHEN** a viewport has a `center` sliver designated
- **AND** `scroll_offset` is `0.0`
- **THEN** the `center` sliver SHALL be positioned at the `anchor` position within the viewport

#### Scenario: Slivers before center
- **WHEN** slivers exist before the `center` sliver in the child list
- **THEN** those slivers SHALL be laid out in reverse order
- **AND** those slivers SHALL appear above (or left of) the center sliver

#### Scenario: Slivers after center
- **WHEN** slivers exist after the `center` sliver in the child list
- **THEN** those slivers SHALL be laid out in forward order
- **AND** those slivers SHALL appear below (or right of) the center sliver

### Requirement: Viewport Anchor Position

`RenderViewport` SHALL support an `anchor` property (0.0 to 1.0) that determines the relative position of the zero scroll offset within the viewport.

#### Scenario: Anchor at 0.0 (default)
- **WHEN** `anchor` is `0.0`
- **THEN** the zero scroll offset SHALL be at the leading edge of the viewport

#### Scenario: Anchor at 0.5 (centered)
- **WHEN** `anchor` is `0.5`
- **THEN** the zero scroll offset SHALL be at the center of the viewport

#### Scenario: Anchor at 1.0
- **WHEN** `anchor` is `1.0`
- **THEN** the zero scroll offset SHALL be at the trailing edge of the viewport

### Requirement: Scroll Offset Correction Handling

`RenderViewport` SHALL handle `scroll_offset_correction` values returned by sliver children by re-running layout with corrected scroll offset.

#### Scenario: Sliver requests scroll correction
- **WHEN** a sliver returns `SliverGeometry` with `scroll_offset_correction = Some(50.0)`
- **THEN** the viewport SHALL apply the correction to `ViewportOffset`
- **AND** the viewport SHALL re-run layout with the corrected scroll offset

### Requirement: Cache Extent for Off-Screen Rendering

`RenderViewport` SHALL support a `cache_extent` property that specifies how much content to render beyond the visible viewport bounds.

#### Scenario: Default cache extent
- **WHEN** `cache_extent` is not specified
- **THEN** the viewport SHALL use a default of `250.0` pixels

#### Scenario: Sliver receives cache extent
- **WHEN** a sliver is within `cache_extent` pixels of the visible area
- **THEN** the sliver SHALL receive non-zero `remaining_cache_extent` in its constraints
- **AND** the sliver SHALL produce geometry with appropriate `cache_extent`

### Requirement: RenderShrinkWrappingViewport

The system SHALL provide `RenderShrinkWrappingViewport` that sizes itself to fit its sliver content.

#### Scenario: Shrink-wrap to content size
- **WHEN** `RenderShrinkWrappingViewport` is given unbounded main-axis constraints
- **THEN** the viewport SHALL size itself to the total `scroll_extent` of all children

#### Scenario: Bounded constraints
- **WHEN** `RenderShrinkWrappingViewport` is given bounded constraints
- **THEN** the viewport SHALL size itself to the minimum of content size and max constraint

### Requirement: RenderAbstractViewport Implementation

`RenderViewport` SHALL implement the `RenderAbstractViewport` trait to support scroll-to-item functionality.

#### Scenario: Get offset to reveal target
- **WHEN** `get_offset_to_reveal(target, alignment, rect, axis)` is called
- **THEN** the viewport SHALL return `RevealedOffset` with scroll offset needed to reveal target
- **AND** the viewport SHALL return the target's rect in viewport coordinates at that offset

## MODIFIED Requirements

### Requirement: SliverGeometry Max Scroll Obstruction Extent

The `SliverGeometry` type SHALL include a `max_scroll_obstruction_extent` field (renamed from `max_scroll_obsolescence`) representing the extent that this sliver obstructs the viewport when pinned.

#### Scenario: Pinned header sliver
- **WHEN** a sliver is a pinned persistent header with height `56.0`
- **THEN** `max_scroll_obstruction_extent` SHALL be `56.0`

#### Scenario: Non-pinned sliver
- **WHEN** a sliver is not pinned (e.g., regular list item)
- **THEN** `max_scroll_obstruction_extent` SHALL be `0.0`
