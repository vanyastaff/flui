# Flui Typography System

A comprehensive font management system for the Flui framework, inspired by Flutter's font loading architecture.

## Features

- **Easy Font Registration**: Multiple ways to add fonts to your application
- **Automatic Caching**: Fonts are loaded once and cached in memory (8.5x faster on subsequent loads)
- **Smart Fallback**: Automatically falls back to similar variants when exact match not found
- **Multiple Sources**: Load fonts from memory, assets, or filesystem
- **Auto-Discovery**: Scan directories and register all fonts automatically
- **Thread-Safe**: Concurrent access with RwLock
- **Backend-Agnostic**: Returns raw TTF/OTF bytes that any renderer can use

## Quick Start

### 1. Auto-Discover Fonts from Directory

The easiest way to add fonts is to let Flui scan and register them automatically:

```rust
use flui_types::typography::FontLoader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Scans assets/fonts and registers all .ttf/.otf files
    FontLoader::register_from_directory("assets/fonts").await?;

    // Fonts are now ready to use!
    Ok(())
}
```

This will:
- Scan all `.ttf` and `.otf` files in the directory
- Extract font family name, weight, and style from metadata
- Automatically group variants into families
- Register everything for immediate use

### 2. Register Individual Font Families

For more control, register font families manually:

```rust
use flui_types::typography::{FontLoader, FontFamily, FileFont, FontWeight, FontStyle};

let mut roboto = FontFamily::new("Roboto");

// Add different weights
roboto.add_font(FileFont::new("fonts/Roboto-Regular.ttf"), FontWeight::W400, FontStyle::Normal);
roboto.add_font(FileFont::new("fonts/Roboto-Bold.ttf"), FontWeight::W700, FontStyle::Normal);
roboto.add_font(FileFont::new("fonts/Roboto-Italic.ttf"), FontWeight::W400, FontStyle::Italic);

// Register the family
FontLoader::register_family(roboto);
```

### 3. Register Single Fonts (Simple Cases)

For single-variant fonts:

```rust
use flui_types::typography::{FontLoader, MemoryFont, FontWeight, FontStyle};

// Embedded font bytes
let font_bytes = include_bytes!("../assets/fonts/MyFont.ttf");

FontLoader::register_font(
    "MyFont",
    MemoryFont::new(font_bytes.to_vec()),
    FontWeight::W400,
    FontStyle::Normal,
);
```

## Loading Fonts

Once registered, load fonts with automatic caching:

```rust
use flui_types::typography::{FontLoader, FontWeight, FontStyle};

// First load: reads from disk/memory and caches
let font_data = FontLoader::load("Roboto", FontWeight::W700, FontStyle::Normal).await?;

// Second load: returns from cache (much faster!)
let font_data = FontLoader::load("Roboto", FontWeight::W700, FontStyle::Normal).await?;

// Use the font data
let bytes = font_data.as_bytes(); // Raw TTF/OTF data
```

**Performance**: Cached loads are ~8.5x faster than initial loads!

## Font Sources

### MemoryFont - Embedded Fonts

Load fonts from memory (e.g., embedded with `include_bytes!`):

```rust
use flui_types::typography::MemoryFont;

let font_bytes = include_bytes!("../assets/fonts/Arial.ttf");
let provider = MemoryFont::new(font_bytes.to_vec());
```

**Pros**: Fast, self-contained, no I/O
**Cons**: Increases binary size

### AssetFont - Asset Bundle

Load fonts from the asset bundle:

```rust
use flui_types::typography::AssetFont;

let provider = AssetFont::new("fonts/Roboto-Regular.ttf")
    .with_package("my_package"); // Optional package
```

**Pros**: Flexible deployment, smaller binaries
**Cons**: Requires asset system

### FileFont - Filesystem

Load fonts directly from filesystem:

```rust
use flui_types::typography::FileFont;

let provider = FileFont::new("C:/Windows/Fonts/arial.ttf");
// or
let provider = FileFont::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
```

**Pros**: No bundling needed, use system fonts
**Cons**: Platform-dependent paths

## Smart Fallback Logic

When an exact font variant isn't found, Flui intelligently falls back:

1. **Exact match** (weight + style)
2. Same weight, Normal style (if Italic was requested)
3. Normal weight (W400), same style
4. Normal weight (W400), Normal style
5. Default font (if set)
6. Any available font in the family

Example:

```rust
// Register only Bold Normal
family.add_font(provider, FontWeight::W700, FontStyle::Normal);

// Request Bold Italic (not registered)
let font = family.load(FontWeight::W700, FontStyle::Italic).await?;
// ✓ Returns Bold Normal automatically
```

## Font Weights

Flui supports all CSS font weights:

```rust
use flui_types::typography::FontWeight;

FontWeight::W100  // Thin
FontWeight::W200  // Extra Light
FontWeight::W300  // Light
FontWeight::W400  // Normal / Regular (default)
FontWeight::W500  // Medium
FontWeight::W600  // Semi Bold
FontWeight::W700  // Bold
FontWeight::W800  // Extra Bold
FontWeight::W900  // Black

// Convenience aliases
FontWeight::THIN
FontWeight::NORMAL
FontWeight::BOLD
```

## Font Styles

```rust
use flui_types::typography::FontStyle;

FontStyle::Normal  // Upright text
FontStyle::Italic  // Slanted text
```

## Cache Management

### Listing Fonts

```rust
// List all registered families
let families = FontLoader::list_families();
println!("Registered: {:?}", families);

// Check if a family exists
if FontLoader::has_family("Roboto") {
    println!("Roboto is available!");
}

// Get family count
let count = FontLoader::family_count();
```

### Clearing Cache

```rust
// Clear font cache but keep registrations
FontLoader::clear_cache();

// Clear everything (mainly for testing)
FontLoader::clear_all();

// Unregister a specific family
FontLoader::unregister_family("Roboto");
```

## Typical Application Setup

Here's a recommended setup for your Flui application:

```rust
use flui_types::typography::{FontLoader, FontFamily, FileFont, FontWeight, FontStyle};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Auto-discover fonts from your assets
    FontLoader::register_from_directory("assets/fonts").await?;

    // 2. Register any custom system fonts
    let mut system_font = FontFamily::new("SystemUI");

    #[cfg(target_os = "windows")]
    system_font.add_font(
        FileFont::new("C:/Windows/Fonts/segoeui.ttf"),
        FontWeight::W400,
        FontStyle::Normal,
    );

    #[cfg(target_os = "macos")]
    system_font.add_font(
        FileFont::new("/System/Library/Fonts/SFNS.ttf"),
        FontWeight::W400,
        FontStyle::Normal,
    );

    FontLoader::register_family(system_font);

    // 3. Run your application
    run_app().await?;

    Ok(())
}
```

## Architecture

### Design Philosophy

The Flui font system follows Flutter's proven architecture:

1. **Provider Pattern**: `FontProvider` trait abstracts font sources
2. **Family Grouping**: Fonts are organized by family with multiple variants
3. **Global Registry**: `FontLoader` singleton manages all fonts
4. **Lazy Loading**: Fonts loaded on-demand and cached
5. **Backend Agnostic**: Returns raw bytes for any rendering backend

### Thread Safety

All font operations are thread-safe:
- `FontLoader` uses `RwLock` for concurrent reads
- `FontData` uses `Arc<Vec<u8>>` for cheap cloning
- Multiple threads can load fonts simultaneously

### Memory Management

Fonts are reference-counted for efficient memory usage:

```rust
pub struct FontData {
    bytes: Arc<Vec<u8>>,  // Shared ownership
    weight: Option<FontWeight>,
    style: Option<FontStyle>,
}
```

Cloning `FontData` is cheap - only the `Arc` is cloned, not the bytes.

## Examples

See `examples/font_loading.rs` for a comprehensive demonstration of all features.

Run it with:
```bash
cargo run -p flui_types --example font_loading
```

## Integration with Text Rendering

The font system integrates seamlessly with Flui's text rendering:

```rust
// Load font
let font_data = FontLoader::load("Roboto", FontWeight::W700, FontStyle::Normal).await?;

// Pass to renderer
let rendered_text = text_renderer.render(
    "Hello, World!",
    &font_data,
    24.0, // font size
)?;
```

## Comparison with Flutter

| Flutter | Flui |
|---------|------|
| `FontLoader` | `FontLoader` |
| `rootBundle.load()` | `FontProvider::load()` |
| `FontWeight.w400` | `FontWeight::W400` |
| `FontStyle.italic` | `FontStyle::Italic` |
| `TextStyle(fontFamily: 'Roboto')` | `FontLoader::load("Roboto", ...)` |

## Dependencies

- `tokio` - Async runtime for font loading
- `ttf-parser` - Font metadata extraction
- `once_cell` - Global singleton pattern
- `tracing` (optional) - Logging font operations

## Future Enhancements

Potential future improvements:

- [ ] Font subsetting for web/mobile
- [ ] Compressed font formats (WOFF2)
- [ ] Variable fonts support
- [ ] Font metrics extraction API
- [ ] Hot-reloading for development
- [ ] Font preloading hints

## Troubleshooting

### Font Not Found

```rust
// Check if registered
if !FontLoader::has_family("MyFont") {
    eprintln!("MyFont is not registered!");
    eprintln!("Available: {:?}", FontLoader::list_families());
}
```

### Slow Font Loading

Fonts are cached after first load. If initial load is slow:
- Use `MemoryFont` for embedded fonts (no I/O)
- Preload fonts during app initialization
- Check disk I/O performance

### Memory Usage

If memory usage is too high:
- Use `FileFont` instead of `MemoryFont` (lazy loading)
- Call `FontLoader::clear_cache()` periodically
- Only register fonts you actually use

## License

Part of the Flui framework. See repository LICENSE for details.
