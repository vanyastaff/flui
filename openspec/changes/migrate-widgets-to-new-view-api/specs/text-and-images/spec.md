# Text and Images Specification

## Purpose

This specification references the detailed text and image widget requirements documented in `crates/flui_widgets/guide/03_text_and_images.md`.

## ADDED Requirements

### Requirement: Text Widget Categories

Text widgets SHALL support simple text, rich text with multiple styles, and selectable text, as documented in guide/03_text_and_images.md.

#### Scenario: Text widget renders single-style text

**GIVEN** a developer needs to display simple styled text
**WHEN** using Text widget
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL use RenderParagraph for rendering
**AND** widget SHALL support TextStyle, TextAlign, TextDirection, overflow, maxLines, textScaler
**AND** widget SHALL support Text() and Text.rich() constructors
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

#### Scenario: RichText widget renders multi-style text

**GIVEN** a developer needs multiple text styles in one widget
**WHEN** using RichText widget with TextSpan tree
**THEN** widget SHALL support TextSpan hierarchy with different styles
**AND** widget SHALL support WidgetSpan for embedded widgets
**AND** widget SHALL use RenderParagraph for rendering
**AND** widget SHALL support textAlign, textDirection, softWrap, overflow, maxLines
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

#### Scenario: SelectableText enables text selection

**GIVEN** a developer needs selectable text
**WHEN** using SelectableText widget
**THEN** widget SHALL support text selection with cursor and controls
**AND** widget SHALL use RenderEditable (readOnly: true)
**AND** widget SHALL support onSelectionChanged callback
**AND** widget SHALL support SelectableText() and SelectableText.rich() constructors
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

---

### Requirement: Text Styling and Theme Integration

Text widgets SHALL integrate with DefaultTextStyle and TextStyle for consistent styling.

#### Scenario: DefaultTextStyle provides inherited text style

**GIVEN** a widget subtree needs consistent text styling
**WHEN** using DefaultTextStyle widget
**THEN** widget SHALL propagate TextStyle to descendant Text widgets
**AND** widget SHALL use InheritedTheme pattern
**AND** widget SHALL support style, textAlign, softWrap, overflow, maxLines parameters
**AND** descendant Text widgets SHALL inherit style unless overridden

#### Scenario: TextStyle defines comprehensive text appearance

**GIVEN** a Text widget needs styling
**WHEN** using TextStyle
**THEN** TextStyle SHALL support color, backgroundColor
**AND** TextStyle SHALL support fontFamily, fontSize, fontWeight, fontStyle
**AND** TextStyle SHALL support decoration, decorationColor, decorationStyle, decorationThickness
**AND** TextStyle SHALL support height, leadingDistribution, letterSpacing, wordSpacing
**AND** TextStyle SHALL support shadows, fontFeatures, fontVariations
**AND** TextStyle SHALL be immutable and composable via copyWith()

---

### Requirement: Image Widget Categories

Image widgets SHALL support loading from assets, network, file, and memory sources, as documented in guide/03_text_and_images.md.

#### Scenario: Image widget loads and displays images

**GIVEN** a developer needs to display an image
**WHEN** using Image widget with ImageProvider
**THEN** widget SHALL use RenderImage for rendering
**AND** widget SHALL support width, height, fit, alignment, repeat parameters
**AND** widget SHALL support color tinting and colorBlendMode
**AND** widget SHALL support filterQuality for scaling
**AND** widget SHALL support Image.asset(), Image.network(), Image.file(), Image.memory() constructors
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

#### Scenario: Image variants provide convenient loading patterns

**GIVEN** a developer needs to load image from specific source
**WHEN** using Image.asset(), Image.network(), Image.file(), or Image.memory()
**THEN** Image.asset SHALL load from AssetBundle with AssetImage provider
**AND** Image.network SHALL load from URL with NetworkImage provider and HTTP caching
**AND** Image.file SHALL load from File with FileImage provider
**AND** Image.memory SHALL decode from Uint8List with MemoryImage provider
**AND** all variants SHALL support loadingBuilder and errorBuilder callbacks

---

### Requirement: Icon Widgets

Icon widgets SHALL render icon fonts and images as Material Design icons.

#### Scenario: Icon widget renders icon font glyphs

**GIVEN** a developer needs to display an icon
**WHEN** using Icon widget with IconData
**THEN** widget SHALL use RenderParagraph for icon font rendering
**AND** widget SHALL support size, color, semanticLabel, textDirection
**AND** widget SHALL inherit IconTheme from IconTheme ancestor
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

#### Scenario: IconTheme provides inherited icon styling

**GIVEN** a widget subtree needs consistent icon styling
**WHEN** using IconTheme widget
**THEN** widget SHALL propagate IconThemeData to descendant Icon widgets
**AND** widget SHALL use InheritedTheme pattern
**AND** IconThemeData SHALL specify color, size, opacity
**AND** descendant Icon widgets SHALL inherit theme unless overridden

#### Scenario: ImageIcon renders image as tinted icon

**GIVEN** a developer needs an image rendered as icon
**WHEN** using ImageIcon widget
**THEN** widget SHALL use RenderImage + RenderShaderMask for tinting
**AND** widget SHALL support ImageProvider, size, color, semanticLabel
**AND** widget SHALL apply color tint via shader mask
**AND** widget SHALL follow patterns documented in guide/03_text_and_images.md

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/03_text_and_images.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 12 text and image widgets

**Text Widgets (5):**
- Text, RichText, SelectableText, DefaultTextStyle
- TextSpan, WidgetSpan (InlineSpan subclasses)

**Image Widgets (4):**
- Image, Image.asset, Image.network, Image.file, Image.memory
- RawImage

**Icon Widgets (3):**
- Icon, IconTheme, ImageIcon

**Supporting Types (not widgets):**
- TextStyle
- IconThemeData
- ImageProvider (AssetImage, NetworkImage, FileImage, MemoryImage)
