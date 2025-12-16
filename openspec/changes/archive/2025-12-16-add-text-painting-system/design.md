# Design: Text Painting System

## Context

FLUI needs a text painting system comparable to Flutter's for:
- Text measurement before rendering
- Rich text with mixed styles
- Text editing support (caret positioning, selection)
- Accessibility (text scaling)

### Constraints

1. **GPU-only rendering** - Must work with glyphon (SDF text)
2. **Thread-safe** - TextPainter must be `Send + Sync`
3. **Zero-cost where possible** - Avoid unnecessary allocations
4. **Flutter API parity** - Similar naming for familiarity

### Stakeholders

- Widget developers (Text, RichText, TextField)
- Layout system (RenderParagraph)
- Accessibility layer (semantics)

## Goals / Non-Goals

### Goals
- Measure text dimensions accurately
- Support styled text spans with nesting
- Enable caret positioning for text editing
- Support system text scaling
- Integrate cleanly with existing Canvas API

### Non-Goals
- Text editing state management (belongs in widgets)
- Font loading/caching (belongs in engine)
- Spell checking, autocomplete (future scope)
- Complex scripts (Arabic, Devanagari) - defer to glyphon

## Decisions

### Decision 1: TextPainter owns layout, not TextSpan

**What:** TextPainter performs layout; TextSpan is pure data.

**Why:** 
- Separation of concerns (data vs behavior)
- TextSpan can be shared/cloned cheaply
- Layout cache lives in TextPainter, not span tree
- Matches Flutter's design

**Alternatives:**
- TextSpan with layout methods - rejected (mixes data and behavior)

### Decision 2: Use glyphon's Buffer directly

**What:** TextPainter wraps glyphon::Buffer for layout.

**Why:**
- glyphon already handles complex text shaping
- Avoid reimplementing Unicode handling
- Get proper font fallback from cosmic-text

**Trade-offs:**
- Couples to glyphon API (acceptable - it's our text backend)
- Need adapter layer for abstraction

### Decision 3: Lazy layout with caching

**What:** Layout computed on first access, cached until invalidated.

**Why:**
- Avoid redundant layout during build phase
- TextPainter.layout() can be called multiple times cheaply
- Matches Flutter's lazy pattern

**Implementation:**
```rust
pub struct TextPainter {
    text: Option<InlineSpan>,
    text_direction: TextDirection,
    // Cached layout
    layout_cache: Option<TextLayout>,
    needs_layout: bool,
}

impl TextPainter {
    pub fn layout(&mut self, min_width: f32, max_width: f32) {
        if !self.needs_layout && self.layout_cache.is_some() {
            return; // Use cache
        }
        // Perform layout...
        self.needs_layout = false;
    }
}
```

### Decision 4: InlineSpan trait hierarchy

**What:** Trait-based span hierarchy matching Flutter.

```rust
pub trait InlineSpan: Send + Sync {
    fn build(&self, builder: &mut SpanBuilder);
    fn visit_children(&self, visitor: &mut dyn TextSpanVisitor) -> bool;
    fn compute_semantics_info(&self) -> Vec<SemanticsInfo>;
}
```

**Why:**
- Extensible for future span types
- Clean separation between TextSpan and PlaceholderSpan
- Enables generic span processing

### Decision 5: TextScaler as trait

**What:** TextScaler is a trait, not a struct.

```rust
pub trait TextScaler: Send + Sync {
    fn scale(&self, font_size: f32) -> f32;
    fn text_scale_factor(&self) -> f32;
}

pub struct LinearTextScaler(f32);
pub struct NoScaling;
```

**Why:**
- Allows custom scaling strategies
- System can provide platform-specific scalers
- Easy to compose (multiply scalers)

## Architecture

### Type Hierarchy

```
flui_types/typography/
├── text_span.rs        # TextSpan data structure
├── strut_style.rs      # StrutStyle for line height
├── text_scaler.rs      # TextScaler trait + impls
├── text_height.rs      # TextHeightBehavior
└── placeholder.rs      # PlaceholderDimensions

flui_painting/
├── text_painter.rs     # TextPainter (layout + paint)
├── inline_span.rs      # InlineSpan trait
└── span_builder.rs     # SpanBuilder for flattening
```

### Data Flow

```
TextSpan (user builds)
    ↓ build()
SpanBuilder (collects styled runs)
    ↓ 
glyphon::Buffer (layout)
    ↓
TextLayout (cached metrics)
    ↓ paint()
DrawCommand::DrawText
    ↓
WgpuPainter → glyphon renderer
```

### Thread Safety

All types are `Send + Sync`:
- TextSpan: immutable data
- TextPainter: mutable but single-owner
- TextLayout: immutable cache
- InlineSpan: trait requires Send + Sync

## Risks / Trade-offs

### Risk 1: glyphon API changes
- **Mitigation:** Wrap in adapter layer
- **Fallback:** Pin glyphon version

### Risk 2: Complex script support
- **Trade-off:** Defer to glyphon/cosmic-text
- **Mitigation:** Test with common scripts (Latin, CJK)

### Risk 3: Performance with large text
- **Trade-off:** Layout caching helps, but re-layout is O(n)
- **Mitigation:** Incremental layout in future version

## Migration Plan

No migration needed - purely additive change.

### Rollout
1. Add foundation types to flui_types
2. Add TextPainter to flui_painting
3. Update RenderParagraph to use TextPainter
4. Update Text widget
5. Add RichText widget

## Open Questions

1. **Should TextPainter own the glyphon::FontSystem?**
   - Current thinking: No, share via Arc from engine

2. **How to handle font fallback configuration?**
   - Defer to glyphon's defaults initially

3. **Should PlaceholderSpan support any widget or specific types?**
   - Start with fixed-size placeholders, expand later
