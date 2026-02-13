//! Text Measurement Example
//!
//! Demonstrates the PlatformTextSystem API for font enumeration,
//! font resolution, metrics, glyph lookup, and text layout.

use flui_platform::{current_platform, Font, FontRun, FontWeight};

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Text System Example ===\n");

    let platform = current_platform().expect("Failed to create platform");
    let text_system = platform.text_system();

    // 1. Enumerate all system fonts
    println!("--- System Fonts ---");
    let font_names = text_system.all_font_names();
    println!("Found {} system fonts", font_names.len());
    for name in font_names.iter().take(10) {
        println!("  - {}", name);
    }
    if font_names.len() > 10 {
        println!("  ... and {} more", font_names.len() - 10);
    }

    // 2. Resolve a font descriptor to a FontId
    println!("\n--- Font Resolution ---");
    let font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Normal,
        ..Default::default()
    };
    match text_system.font_id(&font) {
        Ok(font_id) => {
            println!("Resolved '{}' to {:?}", font.family, font_id);

            // 3. Get font metrics
            println!("\n--- Font Metrics ---");
            let metrics = text_system.font_metrics(font_id);
            println!("  Units per em: {}", metrics.units_per_em);
            println!("  Ascent: {:.1}", metrics.ascent);
            println!("  Descent: {:.1}", metrics.descent);
            println!("  Line gap: {:.1}", metrics.line_gap);
            println!("  Cap height: {:.1}", metrics.cap_height);
            println!("  x-height: {:.1}", metrics.x_height);

            // 4. Glyph lookup
            println!("\n--- Glyph Lookup ---");
            for ch in ['A', 'a', '0', ' ', '!'] {
                match text_system.glyph_for_char(font_id, ch) {
                    Some(glyph_id) => println!("  '{}' -> {:?}", ch, glyph_id),
                    None => println!("  '{}' -> (no glyph)", ch),
                }
            }

            // 5. Text layout
            println!("\n--- Text Layout ---");
            let texts = ["Hello, World!", "FLUI Framework", ""];
            for text in &texts {
                let runs = vec![FontRun {
                    font_id,
                    len: text.len(),
                }];
                let layout = text_system.layout_line(text, 16.0, &runs);
                println!(
                    "  \"{}\" @ 16px -> width: {:.1}, ascent: {:.1}, descent: {:.1}",
                    text, layout.width, layout.ascent, layout.descent
                );
            }

            // 6. Different font sizes
            println!("\n--- Size Scaling ---");
            let text = "Hello";
            for size in [8.0, 12.0, 16.0, 24.0, 48.0] {
                let runs = vec![FontRun {
                    font_id,
                    len: text.len(),
                }];
                let layout = text_system.layout_line(text, size, &runs);
                println!(
                    "  \"{}\" @ {:.0}px -> width: {:.1}",
                    text, size, layout.width
                );
            }
        }
        Err(e) => {
            println!("Failed to resolve font '{}': {}", font.family, e);
        }
    }

    // 7. Try a bold variant
    println!("\n--- Bold Variant ---");
    let bold_font = Font {
        family: "Segoe UI".to_string(),
        weight: FontWeight::Bold,
        ..Default::default()
    };
    match text_system.font_id(&bold_font) {
        Ok(bold_id) => {
            let metrics = text_system.font_metrics(bold_id);
            println!(
                "  Bold ascent: {:.1}, descent: {:.1}",
                metrics.ascent, metrics.descent
            );
        }
        Err(e) => println!("  Failed: {}", e),
    }

    println!("\n=== Text System Example Complete ===");
}
