//! Text Measurement Example
//!
//! Demonstrates the PlatformTextSystem API for measuring text dimensions.
//!
//! This example shows how to:
//! - Get the default system font
//! - Enumerate available system fonts
//! - Measure text bounds at different font sizes
//! - Handle Unicode text (emoji, CJK, RTL scripts)
//!
//! Usage:
//!   cargo run --example text_measurement

use flui_platform::current_platform;
use flui_types::geometry::px;

fn main() -> anyhow::Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Get platform and text system
    // Note: We don't call run() to avoid blocking - just demonstrate the API
    let platform = current_platform()?;
    let text_system = platform.text_system();

    tracing::info!("Text system initialized successfully");

    println!("=== Platform Text System Demo ===");
    println!("Platform: {}", platform.name());
    println!();

    // 1. Get default font
    println!("1. Default System Font:");
    let default_font = text_system.default_font_family();
    println!("   {}", default_font);
    println!();

    // 2. Enumerate system fonts
    println!("2. Available System Fonts:");
    let fonts = text_system.enumerate_system_fonts();
    for (i, font) in fonts.iter().enumerate().take(5) {
        println!("   {}. {}", i + 1, font);
    }
    if fonts.len() > 5 {
        println!("   ... and {} more", fonts.len() - 5);
    }
    println!();

    // 3. Measure text at different sizes
    println!("3. Text Measurement (\"Hello, World!\"):");
    let text = "Hello, World!";
    for font_size in [12.0, 16.0, 24.0, 32.0] {
        let bounds = text_system.measure_text(text, &default_font, font_size);
        println!(
            "   {}px: {:.1}w × {:.1}h",
            font_size,
            bounds.width().0,
            bounds.height().0
        );
    }
    println!();

    // 4. Measure different text lengths
    println!("4. Width Scaling (16px font):");
    let font_size = 16.0;
    for text in ["A", "ABC", "Hello", "Hello, World!"] {
        let bounds = text_system.measure_text(text, &default_font, font_size);
        println!(
            "   {:20} → {:.1}px wide",
            format!("\"{}\"", text),
            bounds.width().0
        );
    }
    println!();

    // 5. Unicode text measurement
    println!("5. Unicode Text Support:");
    let unicode_samples = vec![
        ("Emoji", "Hello 👋 World 🌍!"),
        ("CJK (Chinese)", "你好世界"),
        ("Cyrillic", "Привет мир"),
        ("Arabic (RTL)", "مرحبا بالعالم"),
        ("Mixed", "Test 测试 тест"),
    ];

    for (description, text) in unicode_samples {
        let bounds = text_system.measure_text(text, &default_font, 16.0);
        println!(
            "   {:20} → {:.1}w × {:.1}h",
            description,
            bounds.width().0,
            bounds.height().0
        );
    }
    println!();

    // 6. Font loading (MVP returns NotImplemented)
    println!("6. Font Loading:");
    match text_system.load_system_font(&default_font) {
        Ok(bytes) => println!("   ✓ Loaded {} bytes", bytes.len()),
        Err(err) => println!("   ⚠ {}", err),
    }
    println!();

    // 7. Glyph shaping (MVP returns empty)
    println!("7. Glyph Shaping:");
    let glyphs = text_system.shape_glyphs("Hello", &default_font, 16.0);
    if glyphs.is_empty() {
        println!("   ⚠ NotImplemented (MVP stub)");
        println!("   (Phase 2 will return positioned glyphs for rendering)");
    } else {
        println!("   ✓ {} glyphs shaped", glyphs.len());
    }
    println!();

    // 8. Demonstrate scaling relationship
    println!("8. Proportional Scaling:");
    let base_text = "Test";
    let base_size = 12.0;
    let base_bounds = text_system.measure_text(base_text, &default_font, base_size);

    println!("   Base: {}px → {:.1}w × {:.1}h", base_size, base_bounds.width().0, base_bounds.height().0);

    for multiplier in [1.5, 2.0, 3.0] {
        let scaled_size = base_size * multiplier;
        let scaled_bounds = text_system.measure_text(base_text, &default_font, scaled_size);
        let width_ratio = scaled_bounds.width().0 / base_bounds.width().0;
        let height_ratio = scaled_bounds.height().0 / base_bounds.height().0;

        println!(
            "   {}x: {}px → {:.1}w × {:.1}h (ratio: {:.2}x width, {:.2}x height)",
            multiplier,
            scaled_size,
            scaled_bounds.width().0,
            scaled_bounds.height().0,
            width_ratio,
            height_ratio
        );
    }
    println!();

    // 9. Empty text handling
    println!("9. Edge Cases:");
    let empty_bounds = text_system.measure_text("", &default_font, 16.0);
    println!(
        "   Empty text: {:.1}w × {:.1}h",
        empty_bounds.width().0,
        empty_bounds.height().0
    );

    let single_space = text_system.measure_text(" ", &default_font, 16.0);
    println!(
        "   Single space: {:.1}w × {:.1}h",
        single_space.width().0,
        single_space.height().0
    );
    println!();

    println!("=== MVP Note ===");
    println!("This is a stub implementation for MVP.");
    println!("Phase 2 will integrate:");
    println!("  • Windows: DirectWrite (IDWriteTextLayout)");
    println!("  • macOS: Core Text (CTLine, CTRun)");
    println!("  • Real glyph positioning for GPU rendering");
    println!();

    println!("✓ Text measurement demo complete!");

    Ok(())
}

impl std::fmt::Display for flui_platform::TextSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            flui_platform::TextSystemError::NotImplemented => {
                write!(f, "NotImplemented (MVP stub)")
            }
            flui_platform::TextSystemError::FontNotFound(name) => {
                write!(f, "Font not found: {}", name)
            }
            flui_platform::TextSystemError::LoadFailed(msg) => {
                write!(f, "Load failed: {}", msg)
            }
            flui_platform::TextSystemError::PlatformError(msg) => {
                write!(f, "Platform error: {}", msg)
            }
        }
    }
}
