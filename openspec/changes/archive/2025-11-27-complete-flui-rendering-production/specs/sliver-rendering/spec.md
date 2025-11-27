# sliver-rendering Spec Deltas

## ADDED Requirements

### Requirement: Sliver Paint Implementation

All sliver RenderObjects SHALL implement proper painting of visible children.

#### Scenario: Paint SliverFillViewport children

- **GIVEN** a RenderSliverFillViewport with children
- **WHEN** the paint phase executes
- **THEN** visible children SHALL be painted at viewport-filling positions
- **AND** scroll offset SHALL be applied correctly
- **AND** only visible children SHALL be painted

#### Scenario: Paint SliverList children efficiently

- **GIVEN** a RenderSliverList with many children
- **WHEN** the paint phase executes
- **THEN** only visible children in viewport SHALL be painted
- **AND** child positions SHALL account for scroll offset
- **AND** painting SHALL achieve 60fps performance

#### Scenario: Paint SliverPrototypeExtentList

- **GIVEN** a RenderSliverPrototypeExtentList with prototype
- **WHEN** layout and paint execute
- **THEN** prototype SHALL be measured once
- **AND** all children SHALL use prototype extent
- **AND** children SHALL be painted at calculated positions

### Requirement: Sliver Axis Support

Sliver RenderObjects SHALL support both vertical and horizontal scrolling axes.

#### Scenario: Layout horizontal SliverFixedExtentList

- **GIVEN** a RenderSliverFixedExtentList with horizontal axis
- **WHEN** layout executes with horizontal constraints
- **THEN** children SHALL be laid out horizontally
- **AND** scroll extent SHALL be calculated in horizontal direction
- **AND** cross-axis SHALL use vertical extent

#### Scenario: Paint horizontal sliver

- **GIVEN** a horizontal sliver with children
- **WHEN** paint executes
- **THEN** children SHALL be painted left-to-right
- **AND** horizontal scroll offset SHALL be applied
- **AND** visual output SHALL match vertical sliver behavior

### Requirement: Sliver Performance

Sliver rendering SHALL meet performance targets for smooth scrolling.

#### Scenario: Achieve 60fps scrolling

- **GIVEN** a sliver list with 1000 items
- **WHEN** scrolling continuously
- **THEN** frame time SHALL be <16ms (60fps)
- **AND** only visible items SHALL be processed
- **AND** GC pressure SHALL be minimal

#### Scenario: Handle large scroll extents

- **GIVEN** a sliver with very large scroll extent
- **WHEN** scrolling to arbitrary position
- **THEN** visible range calculation SHALL be O(1)
- **AND** no performance degradation SHALL occur
- **AND** memory usage SHALL be constant
