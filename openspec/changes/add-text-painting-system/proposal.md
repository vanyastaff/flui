# Change: Add Text Painting System

## Why

FLUI currently lacks essential text rendering infrastructure that Flutter provides in its `painting` module. Without `TextPainter`, `TextSpan`, and related types, applications cannot:
- Measure text dimensions before rendering
- Create rich text with mixed styles (bold, italic, colors)
- Handle text layout with proper line breaking
- Support accessibility text scaling
- Implement text editing and selection

This is a critical gap that blocks development of any text-heavy applications.

**Note:** Font and image loading/caching is already handled by `flui_assets` crate:
- `FontAsset` / `FontData` - Font loading from file/memory
- `ImageAsset` - Image loading with decoding
- `AssetCache` - TinyLFU caching with smart eviction
- `AssetRegistry` - Global registry with async loading

This proposal focuses only on **text layout and measurement**, not asset management.

## What Changes

### New Capabilities

1. **TextPainter** - Core text measurement and rendering
   - Layout text with constraints
   - Measure text dimensions (width, height, baseline)
   - Get caret positions for text editing
   - Handle text direction (LTR/RTL)
   - **Uses FontData from flui_assets**

2. **TextSpan** - Rich text building blocks
   - Styled text segments with TextStyle
   - Nested spans for complex formatting
   - Gesture recognizers for tappable text
   - Text semantics for accessibility

3. **StrutStyle** - Line height control
   - Force consistent line heights
   - Cross-font baseline alignment
   - Leading distribution control

4. **TextScaler** - Accessibility scaling
   - System-wide text scaling factor
   - Per-widget scale overrides
   - Non-linear scaling for large text

5. **InlineSpan / PlaceholderSpan** - Inline widgets
   - Embed widgets within text flow
   - Placeholder sizing and alignment

### Architecture

```
FontAsset (flui_assets)
    ↓ load()
FontData (cached)
    ↓
TextSpan (rich text tree)
    ↓ build()
InlineSpan[] (flattened spans)
    ↓ layout()
TextPainter (measurement + caching)
    ↓ paint()
Canvas (DrawCommand::DrawText)
    ↓
glyphon (GPU text rendering)
```

### Integration Points

- **flui_assets**: FontData for font bytes (already exists)
- **flui_painting**: TextPainter, InlineSpan
- **flui_types/typography**: TextSpan, StrutStyle, TextScaler (data types)
- **flui_rendering**: RenderParagraph uses TextPainter
- **flui_widgets**: Text, RichText widgets

## Impact

- Affected specs: None (new capability)
- Affected crates:
  - `flui_painting` - Add TextPainter, InlineSpan
  - `flui_types` - Add TextSpan, StrutStyle, TextScaler
- Dependencies: 
  - glyphon (already in use by flui_engine)
  - flui_assets (FontData for font bytes)
- **NOT breaking** - purely additive

## Alternatives Considered

1. **Minimal text only** - Just DrawCommand::DrawText without measurement
   - Rejected: Cannot implement text editing or layout

2. **Direct glyphon integration** - Use glyphon types directly
   - Rejected: Couples rendering details to painting API

3. **Full Flutter port** - Port all text types including TextEditingController
   - Rejected: Too large scope; editing belongs in widgets layer

4. **Add ImageCache/ImageProvider**
   - Not needed: `flui_assets` already provides this via `AssetCache<ImageAsset>`

## Success Criteria

- [ ] TextPainter can measure text and return dimensions
- [ ] TextSpan supports nested styled text
- [ ] RenderParagraph can use TextPainter for layout
- [ ] Text widget renders with proper line breaking
- [ ] Accessibility scaling works via TextScaler
- [ ] Integration with flui_assets FontData works
