# Tasks: Add Text Painting System

## 1. Foundation Types (flui_types/typography)

- [ ] 1.1 Add `TextSpan` struct with text, style, children, recognizer
- [ ] 1.2 Add `InlineSpanSemanticsInformation` for accessibility
- [ ] 1.3 Add `StrutStyle` struct for line height control
- [ ] 1.4 Add `TextScaler` trait and implementations (Linear, NoScaling)
- [ ] 1.5 Add `TextWidthBasis` enum (parent, longestLine)
- [ ] 1.6 Add `TextHeightBehavior` struct
- [ ] 1.7 Add `PlaceholderDimensions` for inline widgets
- [ ] 1.8 Update typography module exports

## 2. InlineSpan Hierarchy (flui_painting)

- [ ] 2.1 Add `InlineSpan` trait (base for all span types)
- [ ] 2.2 Add `TextSpanVisitor` trait for span traversal
- [ ] 2.3 Add `PlaceholderSpan` for inline widget placeholders
- [ ] 2.4 Implement `InlineSpan` for `TextSpan`
- [ ] 2.5 Add span accumulator for building flat list

## 3. TextPainter Core (flui_painting)

- [ ] 3.1 Create `text_painter.rs` module
- [ ] 3.2 Add `TextPainter` struct with configuration
- [ ] 3.3 Implement `layout(min_width, max_width)` method
- [ ] 3.4 Implement dimension getters (width, height, min/max intrinsic width)
- [ ] 3.5 Implement `get_offset_for_caret(TextPosition)` for cursor positioning
- [ ] 3.6 Implement `get_position_for_offset(Offset)` for hit testing
- [ ] 3.7 Implement `get_line_metrics()` for line-level info
- [ ] 3.8 Implement `paint(canvas, offset)` method
- [ ] 3.9 Add layout caching (invalidate on text/style changes)

## 4. Text Measurement

- [ ] 4.1 Implement `compute_line_metrics()` using glyphon
- [ ] 4.2 Implement `get_boxes_for_selection(start, end)`
- [ ] 4.3 Implement `get_word_boundary(TextPosition)`
- [ ] 4.4 Implement `compute_distance_to_actual_baseline()`
- [ ] 4.5 Add intrinsic width calculation helpers

## 5. Text Direction & Alignment

- [ ] 5.1 Handle `TextDirection` (LTR/RTL) in layout
- [ ] 5.2 Implement `TextAlign` positioning
- [ ] 5.3 Handle bidirectional text (mixed LTR/RTL)
- [ ] 5.4 Implement `text_direction` detection from content

## 6. Accessibility Integration

- [ ] 6.1 Implement `TextScaler` application in layout
- [ ] 6.2 Add semantic label extraction from TextSpan
- [ ] 6.3 Implement `compute_semantics_information()`
- [ ] 6.4 Handle `PlaceholderSpan` semantics

## 7. Tests

- [ ] 7.1 Unit tests for TextSpan building and iteration
- [ ] 7.2 Unit tests for StrutStyle merging
- [ ] 7.3 Unit tests for TextScaler implementations
- [ ] 7.4 Integration tests for TextPainter layout
- [ ] 7.5 Tests for caret positioning accuracy
- [ ] 7.6 Tests for hit testing (offset → position)
- [ ] 7.7 Tests for RTL text handling
- [ ] 7.8 Benchmark text layout performance

## 8. Documentation

- [ ] 8.1 Document TextPainter usage with examples
- [ ] 8.2 Document TextSpan rich text patterns
- [ ] 8.3 Document accessibility features (TextScaler)
- [ ] 8.4 Add integration guide for RenderParagraph
- [ ] 8.5 Update flui_painting README

## 9. Integration

- [ ] 9.1 Update flui_painting lib.rs exports
- [ ] 9.2 Add to flui_painting prelude
- [ ] 9.3 Verify glyphon integration works
- [ ] 9.4 Create example demonstrating rich text

## Dependencies

- glyphon (already available via flui_engine)
- flui_types/typography (TextStyle already exists)
