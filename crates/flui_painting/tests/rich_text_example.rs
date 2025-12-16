//! Rich Text Example
//!
//! This module demonstrates advanced text painting features including:
//! - Multi-style text spans
//! - Cursor positioning and hit testing
//! - Text selection
//! - RTL and bidirectional text
//! - Line metrics

use flui_painting::text_layout::detect_text_direction;
use flui_painting::{DisplayListCore, TextPainter};
use flui_types::geometry::Offset;
use flui_types::styling::Color;
use flui_types::typography::{
    FontStyle, FontWeight, InlineSpan, TextAlign, TextDirection, TextPosition, TextSpan, TextStyle,
};

// ============================================================================
// Example: Basic Rich Text
// ============================================================================

/// Demonstrates creating rich text with multiple styles.
#[test]
fn example_rich_text_basic() {
    // Create a text span with mixed styles using children
    let span = TextSpan::new("Hello, ")
        .with_style(
            TextStyle::new()
                .with_font_size(16.0)
                .with_color(Color::BLACK),
        )
        .with_child(
            // Bold text
            TextSpan::new("World").with_style(TextStyle::new().with_font_weight(FontWeight::BOLD)),
        )
        .with_child(
            // Regular text
            TextSpan::new("! This is "),
        )
        .with_child(
            // Italic text
            TextSpan::new("italic").with_style(TextStyle::new().with_font_style(FontStyle::Italic)),
        )
        .with_child(
            // Colored text
            TextSpan::new(" and colored")
                .with_style(TextStyle::new().with_color(Color::rgb(255, 0, 0))),
        )
        .with_child(TextSpan::new(" text."));

    // Create text painter
    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr)
        .with_text_align(TextAlign::Left);

    // Layout with max width
    painter.layout(0.0, 300.0);

    // Verify layout computed
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);

    println!("Rich text size: {:?}", painter.size());
}

// ============================================================================
// Example: Cursor Positioning
// ============================================================================

/// Demonstrates cursor positioning for text editing.
#[test]
fn example_cursor_positioning() {
    let span = TextSpan::new("Hello, World!").with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    // Get cursor position at different offsets
    let positions = [0, 5, 7, 13];
    for offset in positions {
        let caret_offset = painter.get_offset_for_caret(TextPosition::upstream(offset));
        println!("Cursor at offset {}: {:?}", offset, caret_offset);
    }

    // Cursor positions should increase with offset
    let pos_0 = painter.get_offset_for_caret(TextPosition::upstream(0));
    let pos_5 = painter.get_offset_for_caret(TextPosition::upstream(5));
    assert!(
        pos_5.dx > pos_0.dx,
        "Cursor should move right with increasing offset"
    );
}

// ============================================================================
// Example: Hit Testing
// ============================================================================

/// Demonstrates converting click positions to text offsets.
#[test]
fn example_hit_testing() {
    let span = TextSpan::new("Click anywhere in this text to get position").with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Simulate clicks at different x positions
    let click_positions = [
        Offset::new(0.0, 8.0),   // Start of text
        Offset::new(50.0, 8.0),  // Middle-ish
        Offset::new(100.0, 8.0), // Further right
    ];

    for click in click_positions {
        let text_pos = painter.get_position_for_offset(click);
        println!("Click at {:?} -> text offset {}", click, text_pos.offset);
    }

    // Clicking at x=0 should give offset near 0
    let start_pos = painter.get_position_for_offset(Offset::new(0.0, 8.0));
    assert!(
        start_pos.offset <= 2,
        "Click at start should be near offset 0"
    );
}

// ============================================================================
// Example: Text Selection
// ============================================================================

/// Demonstrates getting selection boxes for highlighting.
#[test]
fn example_text_selection() {
    let span = TextSpan::new("Select some text here").with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Get selection boxes for "some" (offsets 7-11)
    let selection_boxes = painter.get_boxes_for_selection(7, 11);

    println!("Selection boxes for 'some':");
    for (i, text_box) in selection_boxes.iter().enumerate() {
        println!("  Box {}: {:?}", i, text_box.rect);
    }

    // Should have at least one selection box
    assert!(!selection_boxes.is_empty(), "Should have selection boxes");

    // Selection box should have positive dimensions
    if let Some(first) = selection_boxes.first() {
        assert!(first.rect.width() > 0.0);
        assert!(first.rect.height() > 0.0);
    }
}

// ============================================================================
// Example: Word Boundary
// ============================================================================

/// Demonstrates getting word boundaries for double-click selection.
#[test]
fn example_word_boundary() {
    let span = TextSpan::new("Double click on a word").with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Get word boundary at position 8 (middle of "click")
    let word_range = painter.get_word_boundary(TextPosition::upstream(8));
    println!(
        "Word at position 8: range {} to {}",
        word_range.start, word_range.end
    );

    // Should capture "click" (positions 7-12)
    // Note: exact boundaries depend on cosmic-text word detection
    assert!(
        word_range.end > word_range.start,
        "Should have valid word range"
    );
}

// ============================================================================
// Example: Line Metrics
// ============================================================================

/// Demonstrates getting per-line metrics.
#[test]
fn example_line_metrics() {
    // Multi-line text
    let span =
        TextSpan::new("This is the first line.\nThis is the second line.\nAnd a third line.")
            .with_style(
                TextStyle::new()
                    .with_font_size(16.0)
                    .with_color(Color::BLACK),
            );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    let metrics = painter.get_line_metrics();

    println!("Line metrics ({} lines):", metrics.len());
    for line in &metrics {
        println!(
            "  Line {}: height={:.1}, width={:.1}, baseline={:.1}",
            line.line_number, line.height, line.width, line.baseline
        );
    }

    // Should have 3 lines
    assert_eq!(metrics.len(), 3, "Should have 3 lines");

    // Each line should have positive dimensions
    for line in &metrics {
        assert!(line.height > 0.0);
        assert!(line.width > 0.0);
    }
}

// ============================================================================
// Example: RTL Text
// ============================================================================

/// Demonstrates right-to-left text handling.
#[test]
fn example_rtl_text() {
    // Arabic text
    let arabic_text = "مرحبا بالعالم";

    // Auto-detect direction
    let detected = detect_text_direction(arabic_text);
    assert_eq!(detected, Some(TextDirection::Rtl), "Should detect RTL");

    let span = TextSpan::new(arabic_text).with_style(
        TextStyle::new()
            .with_font_size(18.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Rtl);

    painter.layout(0.0, 300.0);

    println!("RTL text size: {:?}", painter.size());

    // Layout should succeed
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

// ============================================================================
// Example: Bidirectional Text
// ============================================================================

/// Demonstrates mixed LTR/RTL text.
#[test]
fn example_bidirectional_text() {
    // Mixed English and Hebrew
    let mixed_text = "Hello שלום World עולם";

    let span = TextSpan::new(mixed_text).with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr); // Base direction LTR

    painter.layout(0.0, 400.0);

    println!("Bidirectional text size: {:?}", painter.size());
    println!("Width: {}, Height: {}", painter.width(), painter.height());

    // Should layout successfully
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

// ============================================================================
// Example: Text Direction Detection
// ============================================================================

/// Demonstrates automatic text direction detection.
#[test]
fn example_direction_detection() {
    let test_cases = [
        ("Hello World", Some(TextDirection::Ltr)),
        ("مرحبا", Some(TextDirection::Rtl)),
        ("שלום", Some(TextDirection::Rtl)),
        ("123", None),                             // Numbers are neutral
        ("   ", None),                             // Whitespace is neutral
        ("Hello مرحبا", Some(TextDirection::Ltr)), // First strong char wins
        ("مرحبا Hello", Some(TextDirection::Rtl)), // First strong char wins
    ];

    for (text, expected) in test_cases {
        let detected = detect_text_direction(text);
        println!("'{}' -> {:?}", text, detected);
        assert_eq!(detected, expected, "Direction mismatch for '{}'", text);
    }
}

// ============================================================================
// Example: Text Alignment
// ============================================================================

/// Demonstrates different text alignments.
#[test]
fn example_text_alignment() {
    let text = "Aligned text";
    let alignments = [
        TextAlign::Left,
        TextAlign::Center,
        TextAlign::Right,
        TextAlign::Justify,
    ];

    for align in alignments {
        let span = TextSpan::new(text).with_style(
            TextStyle::new()
                .with_font_size(16.0)
                .with_color(Color::BLACK),
        );

        let mut painter = TextPainter::new()
            .with_text(InlineSpan::new(span))
            .with_text_direction(TextDirection::Ltr)
            .with_text_align(align);

        painter.layout(0.0, 300.0);

        println!("{:?} alignment - width: {:.1}", align, painter.width());
    }
}

// ============================================================================
// Example: Max Lines and Ellipsis
// ============================================================================

/// Demonstrates text truncation with ellipsis.
#[test]
fn example_max_lines_ellipsis() {
    let long_text = "This is a very long text that should be truncated \
                     after a certain number of lines. It keeps going and going \
                     to demonstrate the ellipsis feature when max lines is set.";

    let span = TextSpan::new(long_text).with_style(
        TextStyle::new()
            .with_font_size(14.0)
            .with_color(Color::BLACK),
    );

    // Without max lines
    let mut painter_full = TextPainter::new()
        .with_text(InlineSpan::new(span.clone()))
        .with_text_direction(TextDirection::Ltr);
    painter_full.layout(0.0, 200.0);

    // With max lines = 2
    let mut painter_truncated = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(2))
        .with_ellipsis(Some("…".to_string()));
    painter_truncated.layout(0.0, 200.0);

    println!("Full text height: {:.1}", painter_full.height());
    println!(
        "Truncated (2 lines) height: {:.1}",
        painter_truncated.height()
    );

    // Truncated should be shorter or equal
    assert!(
        painter_truncated.height() <= painter_full.height(),
        "Truncated text should not be taller than full text"
    );
}

// ============================================================================
// Example: Accessibility Scaling
// ============================================================================

/// Demonstrates text scaling for accessibility.
#[test]
fn example_accessibility_scaling() {
    let span = TextSpan::new("Accessible text").with_style(
        TextStyle::new()
            .with_font_size(16.0)
            .with_color(Color::BLACK),
    );

    let scale_factors = [1.0, 1.25, 1.5, 2.0];

    for scale in scale_factors {
        let mut painter = TextPainter::new()
            .with_text(InlineSpan::new(span.clone()))
            .with_text_direction(TextDirection::Ltr)
            .with_text_scale_factor(scale);

        painter.layout(0.0, 400.0);

        println!("Scale {:.2}x: size {:?}", scale, painter.size());
    }
}

// ============================================================================
// Example: Painting to Canvas
// ============================================================================

/// Demonstrates painting text to a canvas.
#[test]
fn example_paint_to_canvas() {
    use flui_painting::Canvas;

    let span = TextSpan::new("Painted text").with_style(
        TextStyle::new()
            .with_font_size(24.0)
            .with_color(Color::rgb(255, 0, 0)), // Red
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(span))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Create canvas and paint
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::new(10.0, 20.0));

    // Finish to get display list
    let display_list = canvas.finish();

    println!("Display list has {} commands", display_list.len());

    // Should have recorded drawing commands
    assert!(!display_list.is_empty(), "Should have paint commands");
}

// ============================================================================
// Example: Nested Text Spans
// ============================================================================

/// Demonstrates deeply nested text spans for complex formatting.
#[test]
fn example_nested_spans() {
    // Create a paragraph with nested formatting
    let paragraph = TextSpan::with_children(vec![
        TextSpan::new("Normal text, "),
        TextSpan::new("bold")
            .with_style(TextStyle::new().with_font_weight(FontWeight::BOLD))
            .with_child(
                TextSpan::new(" and bold-italic").with_style(
                    TextStyle::new()
                        .with_font_weight(FontWeight::BOLD)
                        .with_font_style(FontStyle::Italic),
                ),
            ),
        TextSpan::new(", then normal again."),
    ])
    .with_style(
        TextStyle::new()
            .with_font_size(14.0)
            .with_color(Color::BLACK),
    );

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(paragraph))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 400.0);

    println!("Nested spans size: {:?}", painter.size());
    assert!(painter.width() > 0.0);
}

// ============================================================================
// Example: Styled Text Builder Pattern
// ============================================================================

/// Demonstrates a clean builder pattern for styled text.
#[test]
fn example_builder_pattern() {
    // Helper to create styled spans
    fn bold(text: &str) -> TextSpan {
        TextSpan::new(text).with_style(TextStyle::new().with_font_weight(FontWeight::BOLD))
    }

    fn italic(text: &str) -> TextSpan {
        TextSpan::new(text).with_style(TextStyle::new().with_font_style(FontStyle::Italic))
    }

    fn colored(text: &str, color: Color) -> TextSpan {
        TextSpan::new(text).with_style(TextStyle::new().with_color(color))
    }

    // Build rich text using helpers
    let rich_text = TextSpan::with_children(vec![
        TextSpan::new("Welcome to "),
        bold("FLUI"),
        TextSpan::new(" - a "),
        italic("Flutter-inspired"),
        TextSpan::new(" UI framework in "),
        colored("Rust", Color::rgb(222, 165, 132)), // Rust orange
        TextSpan::new("!"),
    ])
    .with_style(TextStyle::new().with_font_size(16.0));

    let mut painter = TextPainter::new()
        .with_text(InlineSpan::new(rich_text))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 500.0);

    println!("Builder pattern result: {:?}", painter.size());
    assert!(painter.width() > 0.0);
}
