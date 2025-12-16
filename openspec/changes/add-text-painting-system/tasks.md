# Tasks: Add Text Painting System

## 1. Foundation Types (flui_types/typography)

- [x] 1.1 Add `TextSpan` struct with text, style, children, recognizer
- [x] 1.2 Add `InlineSpanSemanticsInformation` for accessibility
- [x] 1.3 Add `StrutStyle` struct for line height control
- [x] 1.4 Add `TextScaler` trait and implementations (Linear, NoScaling)
- [x] 1.5 Add `TextWidthBasis` enum (parent, longestLine)
- [x] 1.6 Add `TextHeightBehavior` struct
- [x] 1.7 Add `PlaceholderDimensions` for inline widgets
- [x] 1.8 Update typography module exports

## 2. InlineSpan Hierarchy (flui_painting)

- [x] 2.1 Add `InlineSpan` trait (base for all span types)
- [x] 2.2 Add `TextSpanVisitor` trait for span traversal
- [x] 2.3 Add `PlaceholderSpan` for inline widget placeholders
- [x] 2.4 Implement `InlineSpan` for `TextSpan`
- [x] 2.5 Add span accumulator for building flat list

## 3. TextPainter Core (flui_painting)

- [x] 3.1 Create `text_painter.rs` module
- [x] 3.2 Add `TextPainter` struct with configuration
- [x] 3.3 Implement `layout(min_width, max_width)` method
- [x] 3.4 Implement dimension getters (width, height, min/max intrinsic width)
- [x] 3.5 Implement `get_offset_for_caret(TextPosition)` for cursor positioning
- [x] 3.6 Implement `get_position_for_offset(Offset)` for hit testing
- [x] 3.7 Implement `get_line_metrics()` for line-level info
- [x] 3.8 Implement `paint(canvas, offset)` method
- [x] 3.9 Add layout caching (invalidate on text/style changes)

## 4. Text Measurement (cosmic-text integration)

- [x] 4.1 Implement text measurement using cosmic-text
- [x] 4.2 Implement `get_boxes_for_selection(start, end)`
- [x] 4.3 Implement `get_word_boundary(TextPosition)`
- [x] 4.4 Implement `compute_distance_to_actual_baseline()`
- [x] 4.5 Add intrinsic width calculation helpers

## 5. Text Direction & Alignment

- [x] 5.1 Handle `TextDirection` (LTR/RTL) in layout
- [x] 5.2 Implement `TextAlign` positioning
- [x] 5.3 Handle bidirectional text (mixed LTR/RTL)
- [x] 5.4 Implement `text_direction` detection from content

## 6. Accessibility Integration

- [x] 6.1 Implement `TextScaler` application in layout
- [x] 6.2 Add semantic label extraction from TextSpan
- [ ] 6.3 Implement `compute_semantics_information()`
- [ ] 6.4 Handle `PlaceholderSpan` semantics

## 7. Tests

- [x] 7.1 Unit tests for TextSpan building and iteration
- [x] 7.2 Unit tests for StrutStyle merging
- [x] 7.3 Unit tests for TextScaler implementations
- [x] 7.4 Integration tests for TextPainter layout
- [x] 7.5 Tests for caret positioning accuracy
- [x] 7.6 Tests for hit testing (offset → position)
- [x] 7.7 Tests for RTL text handling
- [ ] 7.8 Benchmark text layout performance

## 8. Documentation

- [x] 8.1 Document TextPainter usage with examples
- [x] 8.2 Document TextSpan rich text patterns
- [x] 8.3 Document accessibility features (TextScaler)
- [ ] 8.4 Add integration guide for RenderParagraph
- [x] 8.5 Update flui_painting README

## 9. Integration

- [x] 9.1 Update flui_painting lib.rs exports
- [x] 9.2 Add to flui_painting prelude
- [x] 9.3 Verify cosmic-text integration works
- [ ] 9.4 Create example demonstrating rich text

## Dependencies

- cosmic-text (added for text shaping/measurement)
- glyphon (in flui_engine for GPU rendering)
- flui_types/typography (TextStyle already exists)

## Summary

**Completed:** 46/51 tasks (90%)

**Remaining:**
- 6.3, 6.4: Full semantics support (for screen readers)
- 7.8: Performance benchmarks
- 8.4: RenderParagraph integration guide
- 9.4: Rich text example

The text painting system is feature-complete for basic use. Remaining tasks are enhancements for accessibility and documentation.
