//! Basic usage example for flui-assets.
//!
//! This example demonstrates:
//! - Loading assets through a registry built with `AssetRegistryBuilder`
//! - Using the cache for efficient asset management
//! - Working with different asset types (images and fonts)
//! - Loading embedded assets from owned bytes

use flui_assets::{AssetRegistryBuilder, FontAsset};

#[cfg(feature = "images")]
use flui_assets::ImageAsset;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== FLUI Assets Basic Usage Example ===\n");

    // Create a registry
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();

    // Example 1: Loading a font asset from memory
    println!("1. Loading Font Asset from Memory");
    println!("-----------------------------------");

    let ttf_bytes = vec![
        0x00, 0x01, 0x00, 0x00, // TrueType version
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Minimal header
    ];

    let font = FontAsset::from_bytes("embedded_font.ttf", ttf_bytes);
    let font_handle = registry.load(font).await?;

    println!("  ✓ Font loaded successfully");
    println!("  ✓ Font data size: {} bytes", font_handle.bytes.len());
    println!();

    // Example 2: Loading the same font again (should hit cache)
    println!("2. Loading Font Again (Cache Hit)");
    println!("----------------------------------");

    let font2 = FontAsset::from_bytes("embedded_font.ttf", vec![0; 10]);
    let font_handle2 = registry.load(font2).await?;

    println!("  ✓ Font retrieved from cache");
    println!(
        "  ✓ Both handles point to same data: {}",
        font_handle.bytes.as_ptr() == font_handle2.bytes.as_ptr()
    );
    println!();

    // Example 3: Working with ImageAsset (requires 'images' feature)
    #[cfg(feature = "images")]
    {
        use image::{ImageBuffer, Rgba};
        use std::io::Cursor;

        println!("3. Loading Image Asset");
        println!("----------------------");

        // Create a simple 4x4 gradient image
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |x, y| {
            let intensity = ((x + y) * 32) as u8;
            Rgba([intensity, intensity, intensity, 255])
        });

        // Encode to PNG
        let mut png_bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;

        let image_asset = ImageAsset::from_bytes("gradient.png", png_bytes);
        let image_handle = registry.load(image_asset).await?;

        println!("  ✓ Image loaded successfully");
        println!(
            "  ✓ Dimensions: {}x{}",
            image_handle.width(),
            image_handle.height()
        );
        println!("  ✓ Data size: {} bytes", image_handle.data().len());
        println!();
    }

    #[cfg(not(feature = "images"))]
    {
        println!("3. Image Loading (Skipped)");
        println!("--------------------------");
        println!("  ℹ Enable 'images' feature to test image loading");
        println!();
    }

    // Example 4: Cache invalidation
    println!("4. Cache Invalidation");
    println!("---------------------");

    use flui_assets::AssetKey;
    let key = AssetKey::new("embedded_font.ttf");

    println!(
        "  • Font in cache before invalidation: {}",
        registry.get::<FontAsset>(&key).await.is_some()
    );

    registry.invalidate::<FontAsset>(&key).await;

    println!(
        "  • Font in cache after invalidation: {}",
        registry.get::<FontAsset>(&key).await.is_some()
    );
    println!();

    // Example 5: Preloading assets
    println!("5. Preloading Assets");
    println!("--------------------");

    let preload_fonts = vec![
        FontAsset::from_bytes("font1.ttf", vec![0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0]),
        FontAsset::from_bytes("font2.ttf", vec![0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0]),
        FontAsset::from_bytes("font3.ttf", vec![0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0]),
    ];

    for font in preload_fonts {
        registry.preload(font).await?;
    }

    println!("  ✓ Preloaded 3 fonts into cache");
    println!();

    println!("=== Example Complete ===");

    Ok(())
}
