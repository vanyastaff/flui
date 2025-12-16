# Text Painting Specification

## ADDED Requirements

### Requirement: TextSpan Rich Text Structure

The system SHALL provide a `TextSpan` type for building rich text with mixed styles.

TextSpan MUST support:
- Text content with associated TextStyle
- Nested child spans for mixed formatting
- Optional gesture recognizer for tappable text
- Optional semantic label for accessibility
- Immutable, cloneable data structure

#### Scenario: Simple styled text
- **GIVEN** a TextSpan with text "Hello" and bold style
- **WHEN** the span is rendered
- **THEN** the text appears in bold

#### Scenario: Nested spans
- **GIVEN** a TextSpan with text "Hello " and child span "World" in red
- **WHEN** the spans are flattened
- **THEN** two styled runs are produced: "Hello " (default) and "World" (red)

#### Scenario: Tappable text
- **GIVEN** a TextSpan with a tap recognizer
- **WHEN** the text area is tapped
- **THEN** the recognizer callback is invoked

---

### Requirement: TextPainter Layout and Measurement

The system SHALL provide a `TextPainter` type for measuring and painting text.

TextPainter MUST support:
- Layout with minimum and maximum width constraints
- Querying dimensions: width, height, baseline offset
- Querying intrinsic widths (min and max)
- Caching layout results until text or constraints change
- Thread-safe operation (`Send + Sync`)

#### Scenario: Measure text width
- **GIVEN** a TextPainter with text "Hello World"
- **WHEN** layout is performed with max_width = 1000.0
- **THEN** width returns the actual text width (< max_width)

#### Scenario: Text wrapping
- **GIVEN** a TextPainter with text "Hello World" and max_width = 50.0
- **WHEN** layout is performed
- **THEN** height accounts for wrapped lines

#### Scenario: Layout caching
- **GIVEN** a TextPainter that has been laid out
- **WHEN** layout is called again with same constraints
- **THEN** cached results are returned without recomputation

---

### Requirement: Caret Positioning

The system SHALL provide caret position calculation for text editing.

TextPainter MUST support:
- `get_offset_for_caret(TextPosition)` → Offset
- `get_position_for_offset(Offset)` → TextPosition
- Accurate positioning at character boundaries
- Support for both LTR and RTL text

#### Scenario: Get caret offset
- **GIVEN** a TextPainter with text "Hello" laid out
- **WHEN** get_offset_for_caret is called for position 2
- **THEN** returns the offset after "He"

#### Scenario: Hit test to position
- **GIVEN** a TextPainter with text "Hello" laid out
- **WHEN** get_position_for_offset is called with a point within "l"
- **THEN** returns TextPosition 2 or 3 (nearest character)

---

### Requirement: Text Selection Boxes

The system SHALL provide selection box calculation for text highlighting.

TextPainter MUST support:
- `get_boxes_for_selection(TextSelection)` → Vec<TextBox>
- Multiple boxes for multi-line selections
- Accurate box boundaries at character edges

#### Scenario: Single line selection
- **GIVEN** a TextPainter with text "Hello World"
- **WHEN** get_boxes_for_selection is called for characters 0-5
- **THEN** returns one TextBox covering "Hello"

#### Scenario: Multi-line selection
- **GIVEN** a TextPainter with wrapped text
- **WHEN** get_boxes_for_selection spans multiple lines
- **THEN** returns multiple TextBox instances, one per line

---

### Requirement: StrutStyle Line Height Control

The system SHALL provide `StrutStyle` for consistent line heights.

StrutStyle MUST support:
- Font family and size specification
- Height multiplier
- Leading distribution (top, bottom, proportional)
- Force strut height flag

#### Scenario: Force consistent line height
- **GIVEN** a paragraph with mixed font sizes and forceStrutHeight = true
- **WHEN** the paragraph is laid out
- **THEN** all lines have the same height based on StrutStyle

#### Scenario: Leading distribution
- **GIVEN** a StrutStyle with leadingDistribution = TextLeadingDistribution::Even
- **WHEN** text is laid out
- **THEN** extra leading is distributed equally above and below

---

### Requirement: TextScaler Accessibility

The system SHALL provide `TextScaler` for accessibility text scaling.

TextScaler MUST support:
- `scale(font_size: f32) -> f32` method
- `text_scale_factor() -> f32` for the base factor
- Linear scaling implementation
- No-scaling implementation for opt-out

#### Scenario: Apply system scale factor
- **GIVEN** a TextScaler with factor 1.5
- **WHEN** scale(16.0) is called
- **THEN** returns 24.0

#### Scenario: Large text non-linear scaling
- **GIVEN** a custom TextScaler that caps scaling at 2x for sizes > 24
- **WHEN** scale(32.0) is called with factor 1.5
- **THEN** returns 64.0 (capped at 2x)

---

### Requirement: InlineSpan Trait

The system SHALL provide an `InlineSpan` trait for extensible text content.

InlineSpan MUST support:
- Building into a span builder
- Visiting child spans
- Computing semantics information
- `Send + Sync` for thread safety

#### Scenario: TextSpan implements InlineSpan
- **GIVEN** a TextSpan instance
- **WHEN** cast to &dyn InlineSpan
- **THEN** all InlineSpan methods are available

#### Scenario: PlaceholderSpan for inline widgets
- **GIVEN** a PlaceholderSpan with 24x24 dimensions
- **WHEN** built into a span builder
- **THEN** a placeholder of that size is reserved in text flow

---

### Requirement: PlaceholderSpan Inline Widgets

The system SHALL provide `PlaceholderSpan` for embedding widgets in text.

PlaceholderSpan MUST support:
- Fixed dimensions (width, height)
- Baseline alignment options
- Semantic label for accessibility

#### Scenario: Inline icon
- **GIVEN** a PlaceholderSpan with 16x16 dimensions and baseline alignment
- **WHEN** embedded in text "Click [icon] here"
- **THEN** the placeholder aligns with text baseline

---

### Requirement: Paint Integration

The system SHALL integrate TextPainter with Canvas painting.

TextPainter MUST support:
- `paint(canvas: &mut Canvas, offset: Offset)` method
- Generate appropriate DrawCommand for text rendering
- Support painting with current canvas transform

#### Scenario: Paint text to canvas
- **GIVEN** a laid out TextPainter
- **WHEN** paint is called with a Canvas and offset (10, 20)
- **THEN** DrawCommand::DrawText is recorded at that offset

#### Scenario: Transformed painting
- **GIVEN** a Canvas with rotation transform applied
- **WHEN** TextPainter.paint is called
- **THEN** text is rendered with the canvas transform
