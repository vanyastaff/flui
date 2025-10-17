# Text Widget Guide

The Text widget is a Flutter-style text display widget for nebula-ui, providing comprehensive text rendering capabilities with rich styling options.

## Overview

The Text widget displays a string of text with a single style. It's designed to mirror Flutter's Text widget API while integrating seamlessly with egui's rendering system.

## Basic Usage

### Simple Text

```rust
use nebula_ui::widgets::primitives::Text;

// Most basic usage
Text::new("Hello World").ui(ui);
```

### Styled Text with Builder

```rust
use nebula_ui::types::typography::{TextStyle, TextAlign, TextOverflow};

Text::builder()
    .data("Hello World")
    .style(TextStyle::headline1())
    .text_align(TextAlign::Center)
    .max_lines(2)
    .overflow(TextOverflow::Ellipsis)
    .ui(ui);
```

## Constructors

### `Text::new(data)`
Creates a simple text widget with default styling.

```rust
Text::new("Simple text").ui(ui);
```

### `Text::builder()`
Creates a builder for more complex configuration (recommended for styled text).

```rust
Text::builder()
    .data("Styled text")
    .style(TextStyle::body().bold())
    .ui(ui);
```

### `Text::rich()` (TODO)
For inline spans with multiple styles (not yet implemented).

## Properties

### Core Properties

#### `data: Option<String>`
The text content to display. Required for simple text.

```rust
Text::builder()
    .data("Content here")
    .ui(ui);
```

#### `style: Option<TextStyle>`
Complete text styling including font, size, color, and decorations.

```rust
Text::builder()
    .data("Styled text")
    .style(TextStyle::headline1())
    .ui(ui);

// Or create custom style
let custom_style = TextStyle::body()
    .with_color(Color::BLUE)
    .with_size(FontSize::LARGE)
    .bold();
```

### Alignment Properties

#### `text_align: TextAlign`
Horizontal text alignment. Default: `TextAlign::Left`

Options:
- `TextAlign::Left` - Align to left edge
- `TextAlign::Right` - Align to right edge
- `TextAlign::Center` - Center horizontally
- `TextAlign::Justify` - Justify text (custom layout)
- `TextAlign::Start` - Align to start (direction-dependent)
- `TextAlign::End` - Align to end (direction-dependent)

```rust
Text::builder()
    .data("Centered text")
    .text_align(TextAlign::Center)
    .ui(ui);
```

#### `text_direction: Option<TextDirection>`
Text directionality for RTL/LTR support.

```rust
Text::builder()
    .data("نص عربي")
    .text_direction(TextDirection::Rtl)
    .text_align(TextAlign::Start)  // Will resolve to Right for RTL
    .ui(ui);
```

### Wrapping and Overflow

#### `soft_wrap: bool`
Whether text should wrap at available width. Default: `true`

```rust
// Single line, no wrapping
Text::builder()
    .data("Long text that won't wrap")
    .soft_wrap(false)
    .overflow(TextOverflow::Ellipsis)
    .ui(ui);
```

#### `overflow: TextOverflow`
How to handle text that exceeds container. Default: `TextOverflow::Clip`

Options:
- `TextOverflow::Clip` - Clip overflowing text
- `TextOverflow::Ellipsis` - Show ellipsis (…)
- `TextOverflow::Fade` - Fade out (not fully implemented)
- `TextOverflow::Visible` - Allow overflow

```rust
Text::builder()
    .data("Text with ellipsis...")
    .overflow(TextOverflow::Ellipsis)
    .ui(ui);
```

#### `max_lines: Option<usize>`
Maximum number of lines to display.

```rust
Text::builder()
    .data("Very long text that will be limited to 2 lines...")
    .max_lines(2)
    .overflow(TextOverflow::Ellipsis)
    .ui(ui);
```

### Scaling

#### `text_scaler: Option<TextScaler>`
Scale factor for text size. Default: no scaling (1.0)

```rust
Text::builder()
    .data("Large scaled text")
    .text_scaler(TextScaler::new(1.5))
    .ui(ui);
```

#### `text_scale_factor: Option<f32>` (Deprecated)
Legacy scaling. Use `text_scaler` instead.

### Accessibility

#### `semantics_label: Option<String>`
Alternative text for screen readers.

```rust
Text::builder()
    .data("👋")
    .semantics_label("Waving hand emoji")
    .ui(ui);
```

#### `semantics_identifier: Option<String>`
Identifier for testing and automation.

```rust
Text::builder()
    .data("Login")
    .semantics_identifier("login_button_text")
    .ui(ui);
```

### Advanced Properties

#### `text_width_basis: TextWidthBasis`
How to measure text width. Default: `TextWidthBasis::LongestLine`

#### `text_height_behavior: Option<TextHeightBehavior>`
Controls height calculation for first/last line.

#### `selection_color: Option<Color>`
Custom selection highlight color (note: egui limitation).

#### `locale: Option<String>`
Locale for internationalization (e.g., "en-US", "ja-JP").

#### `key: Option<egui::Id>`
Widget identifier for state persistence.

```rust
Text::builder()
    .key("my_text")
    .data("Stateful text")
    .ui(ui);
```

## Text Styles

### Predefined Styles

```rust
use nebula_ui::types::typography::TextStyle;

// Headlines
TextStyle::headline1()  // 32px, bold
TextStyle::headline2()  // 24px, bold
TextStyle::headline3()  // 20px, semi-bold

// Body text
TextStyle::body()       // 14px, normal
TextStyle::body_large() // 16px, normal
TextStyle::body_small() // 12px, normal

// Special styles
TextStyle::button()     // 14px, medium, letter-spacing
TextStyle::caption()    // 12px, normal, tight
TextStyle::label()      // 12px, medium, letter-spacing
TextStyle::code()       // 12px, monospace, relaxed

// Display styles
TextStyle::display1()   // 57px, normal
TextStyle::display2()   // 45px, normal
TextStyle::display3()   // 36px, normal
```

### Custom Styling

```rust
// Start from base style and modify
let custom = TextStyle::body()
    .with_color(Color::BLUE)
    .with_size(FontSize::LARGE)
    .bold()
    .italic()
    .with_decoration(TextDecoration::Underline);

// Or create from scratch
let custom = TextStyle::new()
    .with_family(FontFamily::monospace())
    .with_size(FontSize::new(18.0))
    .with_weight(FontWeight::Bold)
    .with_color(Color::from_rgb(255, 100, 50));
```

## Examples

### Basic Examples

```rust
// Simple text
Text::new("Hello World").ui(ui);

// Colored text
Text::builder()
    .data("Blue text")
    .style(TextStyle::body().with_color(Color::BLUE))
    .ui(ui);

// Centered headline
Text::builder()
    .data("Centered Title")
    .style(TextStyle::headline1())
    .text_align(TextAlign::Center)
    .ui(ui);
```

### Multi-line Text

```rust
// Wrapped text with max lines
Text::builder()
    .data("This is a long paragraph that will wrap to multiple lines. \
           But we'll limit it to only 3 lines with an ellipsis at the end.")
    .max_lines(3)
    .overflow(TextOverflow::Ellipsis)
    .ui(ui);
```

### Bold and Italic

```rust
// Bold
Text::builder()
    .data("Bold text")
    .style(TextStyle::body().bold())
    .ui(ui);

// Italic
Text::builder()
    .data("Italic text")
    .style(TextStyle::body().italic())
    .ui(ui);

// Both
Text::builder()
    .data("Bold and italic")
    .style(TextStyle::body().bold().italic())
    .ui(ui);
```

### Scaled Text

```rust
let mut scale_factor = 1.0;

ui.add(egui::Slider::new(&mut scale_factor, 0.5..=3.0));

Text::builder()
    .data("Scaled text")
    .text_scaler(TextScaler::new(scale_factor))
    .ui(ui);
```

### Different Alignments

```rust
// Left aligned (default)
Text::builder()
    .data("Left")
    .text_align(TextAlign::Left)
    .ui(ui);

// Centered
Text::builder()
    .data("Center")
    .text_align(TextAlign::Center)
    .ui(ui);

// Right aligned
Text::builder()
    .data("Right")
    .text_align(TextAlign::Right)
    .ui(ui);
```

### Monospace/Code Text

```rust
Text::builder()
    .data("fn main() { println!(\"Hello\"); }")
    .style(TextStyle::code())
    .ui(ui);
```

## Validation

The Text widget includes validation for configuration errors:

```rust
let result = Text::builder()
    .data("Test")
    .max_lines(0)  // Invalid!
    .build(ui);

match result {
    Ok(response) => { /* rendered successfully */ }
    Err(msg) => eprintln!("Validation error: {}", msg),
}
```

Common validation errors:
- Missing data/text_span
- `max_lines` set to 0
- Invalid `text_scale_factor` (negative, NaN, infinite)
- Both data and text_span provided

## Widget Trait Implementation

The Text widget implements `egui::Widget` and `nebula_ui::WidgetExt`:

```rust
// Can be used with egui's add()
ui.add(Text::new("Hello"));

// Or directly
Text::new("Hello").ui(ui);

// With validation
Text::builder()
    .data("Test")
    .build_ui(ui)?;  // Returns Result
```

## Comparison with Flutter

| Flutter Property | nebula-ui Property | Notes |
|-----------------|-------------------|-------|
| `Text(String)` | `Text::new(String)` | Simple constructor |
| `Text.rich(InlineSpan)` | `Text::rich()` | Not yet implemented |
| `style` | `style` | Complete TextStyle support |
| `textAlign` | `text_align` | All alignment modes |
| `textDirection` | `text_direction` | RTL/LTR support |
| `softWrap` | `soft_wrap` | Text wrapping |
| `overflow` | `overflow` | Overflow handling |
| `textScaleFactor` | `text_scale_factor` | Deprecated, use `text_scaler` |
| `textScaler` | `text_scaler` | Preferred scaling method |
| `maxLines` | `max_lines` | Line limit |
| `semanticsLabel` | `semantics_label` | Accessibility |
| `textWidthBasis` | `text_width_basis` | Width calculation |
| `textHeightBehavior` | `text_height_behavior` | Height calculation |
| `selectionColor` | `selection_color` | Selection highlight |

## Implementation Details

### Rendering

The Text widget uses egui's `RichText` and `Label` widgets internally for rendering, providing:
- Proper text shaping and layout
- Font rendering
- Color application
- Wrapping and truncation

### Performance

- Text is measured and shaped using egui's font system
- Galley caching is handled by egui
- Minimal overhead for simple text
- Efficient rendering with egui's retained mode

### Limitations

Current limitations:
1. `Text.rich()` not yet implemented (inline spans)
2. `selection_color` has no effect (egui limitation)
3. `TextOverflow::Fade` not fully implemented
4. `max_lines` is approximated using height constraints
5. `TextAlign::Justify` requires custom implementation

## Future Enhancements

Planned features:
- [ ] InlineSpan support for rich text
- [ ] Custom text decorations (underline, strikethrough)
- [ ] Better overflow handling
- [ ] More precise max_lines implementation
- [ ] Letter spacing support
- [ ] Word spacing support
- [ ] Text shadows

## See Also

- [TextStyle](../src/types/typography/text_style.rs) - Text styling configuration
- [TextAlign](../src/types/typography/text.rs) - Alignment options
- [TextOverflow](../src/types/typography/text.rs) - Overflow handling
- [Container Widget](WRITING_WIDGETS_AND_CONTROLLERS.md) - Layout container
- [Flutter Text Widget](https://api.flutter.dev/flutter/widgets/Text-class.html) - Original inspiration
