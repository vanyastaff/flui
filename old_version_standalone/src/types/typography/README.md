# Typography Types

Comprehensive text styling system for nebula-ui, providing type-safe font configuration with full Color integration.

## Overview

The typography module provides a complete type system for text styling, combining font properties, colors, decorations, and text behaviors. All types follow idiomatic Rust patterns with `impl Into<T>` for maximum flexibility.

## Available Types

### Font Properties

- **[`FontFamily`]** - Font family specification (serif, sans-serif, monospace, custom)
- **[`FontSize`]** - Type-safe font size in points with common constants
- **[`FontWeight`]** - Font weight from Thin (100) to Black (900)
- **[`LineHeight`]** - Line height configuration (normal, tight, relaxed, custom)

### Text Styling

- **[`TextStyle`]** - Complete text style combining all properties
- **[`TextDecoration`]** - Text decorations (underline, strikethrough, overline)

### Text Layout & Behavior

- **[`TextAlign`]** - Horizontal text alignment
- **[`TextDirection`]** - Text directionality (LTR, RTL)
- **[`TextOverflow`]** - Overflow handling (clip, ellipsis, fade)
- **[`TextHeightBehavior`]** - Height calculation behavior
- **[`TextScaler`]** - Text scaling configuration

## Quick Examples

### Basic Text Style

```rust
use nebula_ui::types::{
    core::Color,
    typography::{TextStyle, FontSize, FontWeight},
};

// Simple style
let style = TextStyle::new()
    .with_size(FontSize::LARGE)
    .with_weight(FontWeight::Bold)
    .with_color(Color::BLACK);

// Using rgb tuples (auto-converted to Color)
let colored = TextStyle::new()
    .with_color((255, 100, 50))  // RGB
    .with_size(FontSize::MEDIUM);
```

### Predefined Styles

```rust
use nebula_ui::types::typography::TextStyle;

// Material Design inspired
let h1 = TextStyle::headline1();      // Large, bold
let h2 = TextStyle::headline2();      // Medium, bold
let h3 = TextStyle::headline3();      // Smaller, semi-bold

// Body text
let body = TextStyle::body();         // Normal body text
let body_large = TextStyle::body_large();
let body_small = TextStyle::body_small();

// Special purpose
let caption = TextStyle::caption();   // Small secondary text
let button = TextStyle::button();     // Button labels
let label = TextStyle::label();       // Form labels
let code = TextStyle::code();         // Monospace code
```

### Font Sizes

```rust
use nebula_ui::types::typography::FontSize;

// Common sizes
let small = FontSize::SMALL;      // 12pt
let medium = FontSize::MEDIUM;    // 16pt
let large = FontSize::LARGE;      // 20pt
let x_large = FontSize::X_LARGE;  // 24pt

// Heading sizes
let h1 = FontSize::H1;  // 32pt
let h2 = FontSize::H2;  // 24pt
let h3 = FontSize::H3;  // 18.72pt
let h4 = FontSize::H4;  // 16pt
let h5 = FontSize::H5;  // 13.28pt
let h6 = FontSize::H6;  // 10.72pt

// Custom size
let custom = FontSize::new(14.5);

// Convert to/from pixels
let size = FontSize::from_pixels(16.0);
assert_eq!(size.pixels(), 16.0);

// Scale
let scaled = FontSize::MEDIUM.scale(1.5);  // 24pt
```

### Font Weights

```rust
use nebula_ui::types::typography::FontWeight;

// Named weights
let thin = FontWeight::Thin;          // 100
let extra_light = FontWeight::ExtraLight;  // 200
let light = FontWeight::Light;        // 300
let normal = FontWeight::Normal;      // 400
let medium = FontWeight::Medium;      // 500
let semi_bold = FontWeight::SemiBold; // 600
let bold = FontWeight::Bold;          // 700
let extra_bold = FontWeight::ExtraBold;    // 800
let black = FontWeight::Black;        // 900

// From numeric value
let custom = FontWeight::from_value(450);

// Get numeric value
assert_eq!(FontWeight::Bold.value(), 700);

// Relative weights
let bolder = FontWeight::Normal.bolder();  // SemiBold
let lighter = FontWeight::Bold.lighter();   // Normal
```

### Font Families

```rust
use nebula_ui::types::typography::FontFamily;

// System fonts
let sans = FontFamily::sans_serif();
let serif = FontFamily::serif();
let mono = FontFamily::monospace();

// Custom fonts
let custom = FontFamily::new("Inter");
let with_fallback = FontFamily::with_fallback("Inter", "sans-serif");

// Check properties
assert!(FontFamily::monospace().is_monospace());
```

### Line Heights

```rust
use nebula_ui::types::typography::LineHeight;

// Named heights
let normal = LineHeight::NORMAL;    // 1.5
let tight = LineHeight::TIGHT;      // 1.25
let relaxed = LineHeight::RELAXED;  // 1.75
let loose = LineHeight::LOOSE;      // 2.0

// Custom line height
let custom = LineHeight::new(1.6);

// Calculate actual height for font size
let height = LineHeight::NORMAL.calculate(16.0);  // 24.0
```

### Text Decorations

```rust
use nebula_ui::types::{
    core::Color,
    typography::{TextStyle, TextDecoration},
};

// Underline
let underlined = TextStyle::new()
    .with_decoration(TextDecoration::Underline)
    .with_decoration_color(Color::BLUE);

// Strikethrough
let strikethrough = TextStyle::new()
    .with_decoration(TextDecoration::LineThrough);

// Multiple decorations
let fancy = TextStyle::new()
    .with_decoration(TextDecoration::UnderlineOverline)
    .with_decoration_color(Color::RED);

// Check decoration properties
assert!(TextDecoration::Underline.has_underline());
assert!(TextDecoration::LineThrough.has_line_through());
assert!(TextDecoration::UnderlineOverline.has_underline());
assert!(TextDecoration::UnderlineOverline.has_overline());
```

### Text Shadows

```rust
use nebula_ui::types::{
    core::{Color, Offset},
    styling::Shadow,
    typography::TextStyle,
};

let shadowed = TextStyle::new()
    .with_color(Color::WHITE)
    .with_shadow(Shadow::new(
        Color::from_rgba(0, 0, 0, 50),
        Offset::new(2.0, 2.0),
        4.0,
    ));
```

### Letter & Word Spacing

```rust
use nebula_ui::types::typography::TextStyle;

let spaced = TextStyle::new()
    .with_letter_spacing(0.5)   // Add 0.5px between letters
    .with_word_spacing(2.0);    // Add 2px between words

// Tight spacing (for headings)
let tight = TextStyle::headline1()
    .with_letter_spacing(-0.5);
```

### Style Variants

```rust
use nebula_ui::types::typography::TextStyle;

let base = TextStyle::body();

// Create variants
let bold = base.bold();
let italic = base.italic();
let larger = base.scale(1.25);

// Change color while keeping other properties
let colored = base.with_different_color((200, 50, 100));
```

## Complete Example

```rust
use nebula_ui::types::{
    core::{Color, Offset},
    styling::Shadow,
    typography::{
        TextStyle, FontFamily, FontSize, FontWeight,
        LineHeight, TextDecoration,
    },
};

let article_title = TextStyle::new()
    .with_family(FontFamily::serif())
    .with_size(FontSize::new(48.0))
    .with_weight(FontWeight::Bold)
    .with_color(Color::from_rgb(20, 20, 40))
    .with_line_height(LineHeight::TIGHT)
    .with_letter_spacing(-0.5);

let article_body = TextStyle::new()
    .with_family(FontFamily::serif())
    .with_size(FontSize::new(18.0))
    .with_weight(FontWeight::Normal)
    .with_color(Color::from_rgb(40, 40, 60))
    .with_line_height(LineHeight::RELAXED);

let code_snippet = TextStyle::new()
    .with_family(FontFamily::monospace())
    .with_size(FontSize::SMALL)
    .with_color(Color::from_rgb(220, 220, 240))
    .with_line_height(LineHeight::RELAXED)
    .with_letter_spacing(0.2);

let link = TextStyle::new()
    .with_color(Color::BLUE)
    .with_decoration(TextDecoration::Underline)
    .with_decoration_color(Color::BLUE);
```

## Text Alignment

```rust
use nebula_ui::types::typography::TextAlign;

let left = TextAlign::Left;
let right = TextAlign::Right;
let center = TextAlign::Center;
let justify = TextAlign::Justify;

// Directional start/end
let start = TextAlign::Start;  // Left in LTR, Right in RTL
let end = TextAlign::End;      // Right in LTR, Left in RTL

// Resolve to concrete alignment
let resolved = TextAlign::Start.resolve(TextDirection::Ltr);
assert_eq!(resolved, TextAlign::Left);
```

## Text Direction

```rust
use nebula_ui::types::typography::TextDirection;

let ltr = TextDirection::Ltr;  // Left-to-right
let rtl = TextDirection::Rtl;  // Right-to-left

// Check direction
assert!(TextDirection::Ltr.is_ltr());
assert!(TextDirection::Rtl.is_rtl());
```

## Text Overflow

```rust
use nebula_ui::types::typography::TextOverflow;

// Clip at boundary
let clip = TextOverflow::Clip;

// Show ellipsis (...)
let ellipsis = TextOverflow::Ellipsis;

// Fade out
let fade = TextOverflow::Fade;

// Render everything (may overflow)
let visible = TextOverflow::Visible;
```

## Text Scaling

```rust
use nebula_ui::types::typography::TextScaler;

// No scaling
let none = TextScaler::NoScaling;

// Linear scaling
let linear = TextScaler::Linear { factor: 1.2 };

// Custom scaling function
let custom = TextScaler::custom(|size| {
    if size < 12.0 {
        size * 1.5
    } else {
        size * 1.2
    }
});

// Apply scaling
let scaled_size = TextScaler::Linear { factor: 1.5 }.scale(16.0);
assert_eq!(scaled_size, 24.0);
```

## Design Principles

### 1. **Type Safety**

All values are strongly typed to prevent errors:
```rust
// Type safe - won't compile with wrong types
let style = TextStyle::new()
    .with_size(FontSize::LARGE)        // FontSize, not f32
    .with_weight(FontWeight::Bold)     // FontWeight, not i32
    .with_color(Color::BLACK);         // Color, not u32

// But flexible with conversions
let flexible = TextStyle::new()
    .with_color((255, 0, 0))           // Auto-converts to Color
    .with_size(FontSize::from_pixels(16.0));
```

### 2. **Immutability with Builders**

All modifications return new instances:
```rust
let base = TextStyle::body();
let bold = base.bold();        // base is unchanged
let italic = base.italic();    // base is unchanged

// Chain modifications
let styled = TextStyle::new()
    .with_size(FontSize::LARGE)
    .with_weight(FontWeight::Bold)
    .with_color(Color::RED)
    .with_italic(true);
```

### 3. **Semantic Constants**

Use meaningful names instead of magic numbers:
```rust
// Good
let h1 = FontSize::H1;
let body = FontSize::MEDIUM;
let normal = FontWeight::Normal;
let tight = LineHeight::TIGHT;

// Avoid
let size = FontSize::new(32.0);
let weight = FontWeight::from_value(400);
```

### 4. **Integration with Core**

Seamless integration with core Color type:
```rust
use nebula_ui::types::{
    core::Color,
    typography::TextStyle,
};

// Direct Color constants
let black_text = TextStyle::new().with_color(Color::BLACK);

// RGB tuples
let rgb_text = TextStyle::new().with_color((255, 100, 50));

// RGBA tuples
let rgba_text = TextStyle::new().with_color((255, 100, 50, 200));

// Hex colors
let hex_text = TextStyle::new().with_color(
    Color::from_hex("#FF6432").unwrap()
);
```

## Common Patterns

### Responsive Typography

```rust
fn responsive_title(viewport_width: f32) -> TextStyle {
    let size = if viewport_width < 600.0 {
        FontSize::new(24.0)
    } else if viewport_width < 1200.0 {
        FontSize::new(36.0)
    } else {
        FontSize::new(48.0)
    };

    TextStyle::headline1().with_size(size)
}
```

### Theme-Based Typography

```rust
struct Typography {
    heading1: TextStyle,
    heading2: TextStyle,
    body: TextStyle,
    caption: TextStyle,
}

impl Typography {
    fn light_theme() -> Self {
        Self {
            heading1: TextStyle::headline1()
                .with_color(Color::from_rgb(20, 20, 40)),
            heading2: TextStyle::headline2()
                .with_color(Color::from_rgb(40, 40, 60)),
            body: TextStyle::body()
                .with_color(Color::from_rgb(60, 60, 80)),
            caption: TextStyle::caption()
                .with_color(Color::from_rgb(120, 120, 140)),
        }
    }

    fn dark_theme() -> Self {
        Self {
            heading1: TextStyle::headline1()
                .with_color(Color::from_rgb(240, 240, 255)),
            heading2: TextStyle::headline2()
                .with_color(Color::from_rgb(220, 220, 235)),
            body: TextStyle::body()
                .with_color(Color::from_rgb(200, 200, 215)),
            caption: TextStyle::caption()
                .with_color(Color::from_rgb(160, 160, 175)),
        }
    }
}
```

### Accessibility

```rust
fn accessible_text_style(user_preference: f32) -> TextStyle {
    TextStyle::body()
        .with_size(FontSize::from_pixels(16.0 * user_preference))
        .with_line_height(LineHeight::RELAXED)  // Better readability
        .with_weight(FontWeight::Normal)
}
```

## Testing

All typography types have comprehensive tests:

```bash
# Test all typography types
cargo test --lib --package nebula-ui types::typography

# Test specific types
cargo test --lib --package nebula-ui types::typography::font
cargo test --lib --package nebula-ui types::typography::text_style
```

## Performance Notes

- All font types are `Copy` (FontSize, FontWeight, LineHeight)
- TextStyle is `Clone` (contains non-Copy types like String, Shadow)
- FontFamily uses `Arc<str>` internally for cheap cloning
- All operations are zero-cost abstractions where possible
- Predefined styles are computed fresh each time (not cached)

## See Also

- **[Core Types](../core/README.md)** - Color and other fundamental types
- **[Styling Types](../styling/README.md)** - Shadow and visual effects
- **[Layout Types](../layout/README.md)** - Text layout and positioning

## Examples

Check the example files for complete usage:
- `examples/comprehensive_demo.rs` - Full showcase
- `examples/demo.rs` - Basic usage
- `examples/extended_demo.rs` - Advanced patterns
