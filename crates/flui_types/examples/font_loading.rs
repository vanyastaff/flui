//! Example demonstrating the font loading system.
//!
//! This shows how framework users can easily add fonts in several ways:
//! 1. Register fonts from a directory (automatic discovery)
//! 2. Register individual font families with variants
//! 3. Register single fonts
//! 4. Load and use fonts with caching

use flui_types::typography::{
    AssetFont, FileFont, FontFamily, FontLoader, FontStyle, FontWeight, MemoryFont,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Flui Font Loading Example ===\n");

    // Example 1: Automatic font discovery from directory
    // This scans assets/fonts for all .ttf/.otf files and registers them automatically
    println!("1. Auto-discovering fonts from assets/fonts...");
    match FontLoader::register_from_directory("crates/flui_engine/assets/fonts").await {
        Ok(count) => println!("   ✓ Registered {} fonts automatically\n", count),
        Err(e) => println!(
            "   ⚠ Could not scan directory: {} (this is OK for demo)\n",
            e
        ),
    }

    // Example 2: Register a font family with multiple variants
    println!("2. Registering a font family with multiple variants...");

    // Create a font family for "Roboto"
    let mut roboto_family = FontFamily::new("Roboto");

    // Add different weights and styles
    // In a real app, these would be actual font files
    let regular_bytes = vec![0x00, 0x01, 0x00, 0x00]; // Minimal TTF signature
    let bold_bytes = vec![0x00, 0x01, 0x00, 0x00];
    let italic_bytes = vec![0x00, 0x01, 0x00, 0x00];

    roboto_family.add_font(
        MemoryFont::new(regular_bytes.clone()),
        FontWeight::W400,
        FontStyle::Normal,
    );

    roboto_family.add_font(
        MemoryFont::new(bold_bytes.clone()),
        FontWeight::W700,
        FontStyle::Normal,
    );

    roboto_family.add_font(
        MemoryFont::new(italic_bytes.clone()),
        FontWeight::W400,
        FontStyle::Italic,
    );

    FontLoader::register_family(roboto_family);
    println!("   ✓ Registered Roboto family with 3 variants\n");

    // Example 3: Register a single font (simple case)
    println!("3. Registering a simple single-variant font...");
    FontLoader::register_font(
        "MyCustomFont",
        MemoryFont::new(regular_bytes.clone()),
        FontWeight::W400,
        FontStyle::Normal,
    );
    println!("   ✓ Registered MyCustomFont\n");

    // Example 4: Register fonts from different sources
    println!("4. Registering fonts from different sources...");

    // From asset bundle
    let mut asset_family = FontFamily::new("AssetFont");
    asset_family.add_font(
        AssetFont::new("fonts/CustomFont-Regular.ttf"),
        FontWeight::W400,
        FontStyle::Normal,
    );
    FontLoader::register_family(asset_family);
    println!("   ✓ Registered AssetFont (from asset bundle)");

    // From file system (using a test font)
    let mut file_family = FontFamily::new("SystemFont");
    file_family.add_font(
        FileFont::new("C:/Windows/Fonts/arial.ttf"),
        FontWeight::W400,
        FontStyle::Normal,
    );
    FontLoader::register_family(file_family);
    println!("   ✓ Registered SystemFont (from file system)\n");

    // Example 5: List all registered fonts
    println!("5. Listing all registered font families...");
    let families = FontLoader::list_families();
    println!("   Found {} font families:", families.len());
    for family in &families {
        println!("     • {}", family);
    }
    println!();

    // Example 6: Load fonts (with automatic caching)
    println!("6. Loading fonts (demonstrates caching)...");

    // First load - reads from provider and caches
    println!("   Loading Roboto Regular (first time - will cache)...");
    let start = std::time::Instant::now();
    match FontLoader::load("Roboto", FontWeight::W400, FontStyle::Normal).await {
        Ok(font_data) => {
            let duration = start.elapsed();
            println!(
                "   ✓ Loaded {} bytes in {:?}",
                font_data.as_bytes().len(),
                duration
            );
        }
        Err(e) => println!("   ✗ Failed to load: {}", e),
    }

    // Second load - returns from cache (much faster)
    println!("   Loading Roboto Regular (second time - from cache)...");
    let start = std::time::Instant::now();
    match FontLoader::load("Roboto", FontWeight::W400, FontStyle::Normal).await {
        Ok(font_data) => {
            let duration = start.elapsed();
            println!(
                "   ✓ Loaded {} bytes in {:?} (cached!)",
                font_data.as_bytes().len(),
                duration
            );
        }
        Err(e) => println!("   ✗ Failed to load: {}", e),
    }
    println!();

    // Example 7: Font fallback behavior
    println!("7. Demonstrating smart font fallback...");

    // Request bold italic, but we only have bold normal
    println!("   Requesting Roboto Bold Italic (not registered)...");
    match FontLoader::load("Roboto", FontWeight::W700, FontStyle::Italic).await {
        Ok(font_data) => {
            println!(
                "   ✓ Got fallback font: {} bytes",
                font_data.as_bytes().len()
            );
            println!("     (Automatically fell back to Bold Normal)");
        }
        Err(e) => println!("   ✗ Failed: {}", e),
    }
    println!();

    // Example 8: Font family information
    println!("8. Getting font family information...");
    if let Some(family) = FontLoader::family("Roboto") {
        println!("   Family: {}", family.name());
        println!("   Variants: {}", family.variant_count());
        println!("   Available variants:");
        for (weight, style) in family.variants() {
            println!("     • {:?} {:?}", weight, style);
        }
    }
    println!();

    // Example 9: Cache management
    println!("9. Cache management...");
    println!("   Current families: {}", FontLoader::family_count());
    println!("   Clearing font cache (keeps registrations)...");
    FontLoader::clear_cache();
    println!("   ✓ Cache cleared");
    println!(
        "   Families still registered: {}",
        FontLoader::family_count()
    );
    println!();

    println!("=== Example Complete ===");
    println!("\nKey takeaways:");
    println!("• Fonts are loaded once and cached automatically");
    println!("• Multiple registration methods for flexibility");
    println!("• Smart fallback when exact variant not found");
    println!("• Thread-safe for concurrent access");
    println!("• Works with any font source (memory, assets, files)");

    Ok(())
}
