# Layout Widgets Specification

## Purpose

This specification references the detailed layout widget requirements documented in `crates/flui_widgets/guide/01_layout_widgets.md`.

## ADDED Requirements

### Requirement: Layout Widget Categories

Layout widgets SHALL be organized into three categories: Basic Layout, Flex Layout, and Positioned Layout, as documented in guide/01_layout_widgets.md.

#### Scenario: Basic layout widgets provide fundamental positioning

**GIVEN** a developer needs to position or size a single child widget
**WHEN** using basic layout widgets (Container, SizedBox, Padding, Center, Align, FittedBox, AspectRatio, ConstrainedBox, LimitedBox, FractionallySizedBox, Baseline, OverflowBox)
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL support declarative macro where applicable
**AND** widget SHALL compose appropriate RenderObject(s)
**AND** widget SHALL follow patterns documented in guide/01_layout_widgets.md

#### Scenario: Flex layout widgets arrange multiple children linearly

**GIVEN** a developer needs to arrange multiple children in a row or column
**WHEN** using flex layout widgets (Row, Column, Flex, Expanded, Flexible, Spacer, Wrap)
**THEN** widget SHALL support children parameter
**AND** widget SHALL support main axis and cross axis alignment
**AND** widget SHALL support spacing convenience methods
**AND** widget SHALL use RenderFlex with appropriate Axis
**AND** widget SHALL follow patterns documented in guide/01_layout_widgets.md

#### Scenario: Positioned layout widgets enable z-ordering and absolute positioning

**GIVEN** a developer needs to overlay or absolutely position children
**WHEN** using positioned layout widgets (Stack, Positioned, IndexedStack)
**THEN** widget SHALL support z-ordering of children
**AND** widget SHALL support absolute positioning via Positioned
**AND** widget SHALL use RenderStack or RenderIndexedStack
**AND** widget SHALL follow patterns documented in guide/01_layout_widgets.md

---

### Requirement: Container Widget Composition

Container widget SHALL compose multiple RenderObjects to provide comprehensive layout control in a single widget.

#### Scenario: Container composes RenderObjects in correct order

**GIVEN** a Container widget with multiple properties (padding, margin, color, decoration, alignment, constraints, transform)
**WHEN** build() method is called
**THEN** RenderObjects SHALL be composed in correct order: margin → constraints → decoration → alignment → padding → child
**AND** each property SHALL only create RenderObject if value is Some
**AND** composition SHALL follow Flutter's Container implementation
**AND** convenience methods SHALL provide common presets (colored, card, outlined, surface, rounded, sized, padded, centered)

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/01_layout_widgets.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 22 layout widgets

**Basic Layout (12):**
- Container, SizedBox, Padding, Center, Align
- FittedBox, AspectRatio, ConstrainedBox, LimitedBox
- FractionallySizedBox, Baseline, OverflowBox

**Flex Layout (7):**
- Row, Column, Flex, Expanded, Flexible, Spacer, Wrap

**Positioned Layout (3):**
- Stack, Positioned, IndexedStack
