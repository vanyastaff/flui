# Implementation Plan: flui-platform MVP - Cross-Platform Support

**Branch**: `dev` | **Date**: 2026-01-26 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/dev/spec.md`

## Summary

Complete flui-platform crate to MVP-ready state with cross-platform support for Windows and macOS. The crate provides platform abstraction for window management, text systems, event handling, display enumeration, async executors, and clipboard operations. Current implementation is 90-100% complete for Windows/macOS but lacks text system integration (critical for rendering) and macOS hardware testing. MVP focuses on completing these blockers and ensuring 70%+ test coverage.

**Technical Approach**: Use native platform APIs (Win32, AppKit) for best control, adopt GPUI's proven patterns (Platform trait, callback registry, type erasure), follow Flutter's modular binding approach, and use W3C-standard events for consistency.

## Technical Context

**Language/Version**: Rust 1.75+ (per constitution requirement, stable toolchain)  
**Primary Dependencies**: 
  - **Core**: `parking_lot 0.12` (fast sync primitives, 2-3x faster than std), `tokio 1.43` (async runtime, LTS until March 2026), `flume 0.11` (MPSC channels for UI thread), `tracing 0.1` (structured logging, mandatory per constitution), `dashmap 6.1` (concurrent HashMap for font cache)
  - **Platform Integration**: `raw-window-handle 0.6` (wgpu surface integration), `waker-fn 1.2` (executor utilities), `ui-events` (W3C event types), `keyboard-types 0.8` (key codes), `dpi 0.1` (DPI scaling)
  - **Windows**: `windows 0.59+` (Win32 API with features: `Win32_Graphics_DirectWrite`, `Win32_Graphics_Direct2D`, `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_System_DataExchange`)
  - **macOS**: `cocoa 0.26+` (AppKit), `cocoa-foundation 0.2`, `core-foundation 0.10`, `core-graphics 0.24`, `core-text 0.24+` (text shaping), `objc 0.2`
  - **Error Handling**: `thiserror 2.0+` (error derive macros for TextSystemError)
  - **Dev/Test**: `tracing-subscriber` (examples/tests), `criterion` (benchmarks)

**Storage**: N/A (in-memory platform state only - no persistence layer)  
**Testing**: `cargo test` with headless mode support (`FLUI_HEADLESS=1`), contract tests for Platform trait compliance, integration tests for cross-crate interactions, criterion benchmarks for performance verification  
**Target Platform**: 
  - **Primary**: Windows 10 (1809+) / Windows 11, macOS 11 (Big Sur)+
  - **Testing**: Headless mode (CI/CD without GPU or display server)
  - **Future**: Linux (X11/Wayland), Android, iOS, Web/WASM
  
**Project Type**: Library crate (`flui-platform`) within monorepo workspace  
**Performance Goals**: 
  - Text measurement: <1ms for <100 char strings (NFR-001)
  - Event dispatch: <5ms from OS to callback (NFR-002)
  - Display enumeration: <10ms with 4+ monitors (NFR-003)
  - Executor spawn: <100µs overhead (NFR-004)
  - Clipboard roundtrip: <1ms for 1KB text (NFR-005)
  - Frame budget: 16.67ms for 60fps (per constitution Principle IX)
  
**Constraints**: 
  - Native platform APIs only (no cross-platform frameworks like SDL/GLFW - we wrap them ourselves)
  - Must integrate with wgpu for GPU rendering (requires raw-window-handle)
  - W3C UI Events specification compliance for cross-platform consistency
  - Zero unsafe code without explicit justification (per constitution)
  - All logging via `tracing` crate, NEVER `println!` (per constitution Principle VI)
  - Lock-free dirty tracking with AtomicBool (per constitution Principle IX)
  
**Scale/Scope**: 
  - MVP: 125 tasks across 10 phases (2-3 weeks estimated)
  - Codebase: ~5K-8K LOC for platform layer
  - Test coverage: ≥70% (per constitution for platform crates)
  - Platforms: 3 implementations (Windows, macOS, Headless) for MVP
  - Future: 6 total platforms (add Linux, Android, iOS, Web)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| **I. Flutter-Inspired Architecture** | ✅ PASS | Platform abstraction follows Flutter's binding pattern with Platform trait providing lifecycle, executors, text system |
| **II. Type Safety First** | ✅ PASS | Strong types throughout: WindowId, DisplayId, typed Units (Pixels, DevicePixels), Arc<dyn Platform> for type erasure |
| **III. Modular Architecture** | ✅ PASS | flui-platform is independent crate with clear dependency boundaries (no upward dependencies) |
| **IV. Test-First for Public APIs** | ✅ PASS | Contract tests (T009), integration tests (T010), test tasks before implementation in all phases |
| **V. Explicit Over Implicit** | ✅ PASS | Platform trait methods explicit, callback registry pattern, no hidden state mutations |
| **VI. Code Quality Standards** | ✅ PASS | Tracing setup (T003), parking_lot for sync, dependencies added (T006-T007), structured logging required |
| **VII. Incremental Development** | ✅ PASS | P1/P2/P3 priorities, 125 tasks with [P] parallel markers, 10 phases with clear gates |
| **VIII. UX Consistency** | ✅ PASS | W3C UI Events (FR-011), Flutter naming (Platform, Window, Display), consistent API patterns |
| **IX. Performance Requirements** | ✅ PASS | Quantified targets (NFR-001 to NFR-005), benchmarks (T116-T120), lock-free dirty tracking |

**Quality Gates Coverage**:
- ✅ **Compilation Gate**: T001 (workspace build), T121 (final build)
- ✅ **Testing Standards**: T002 (baseline), T009 (contract), T103-T110 (coverage ≥70%), T124 (final suite)
- ✅ **Documentation Gate**: T008, T044, T070, T111-T115 (rustdoc for all public APIs)
- ✅ **Performance Gate**: T116-T120 (benchmarks), lock-free atomics, no hot-path allocations

**Overall Result**: ✅ **ALL PRINCIPLES SATISFIED** - No constitution violations, ready for implementation

## Project Structure

### Documentation (this feature)

```text
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (flui-platform crate)

```text
crates/flui-platform/
├── src/
│   ├── lib.rs                      # Public API re-exports, current_platform() function
│   ├── traits/                     # Core abstractions
│   │   ├── mod.rs                 # Module organization
│   │   ├── platform.rs            # Platform trait (central abstraction)
│   │   ├── window.rs              # PlatformWindow trait
│   │   ├── display.rs             # PlatformDisplay trait
│   │   ├── capabilities.rs        # PlatformCapabilities trait
│   │   ├── lifecycle.rs           # PlatformLifecycle trait
│   │   └── input.rs               # Input handling types
│   ├── platforms/                  # Platform implementations
│   │   ├── mod.rs                 # Platform module organization
│   │   ├── windows/               # Windows (Win32 API)
│   │   │   ├── mod.rs
│   │   │   ├── platform.rs        # WindowsPlatform implementation
│   │   │   ├── window.rs          # Win32 window management
│   │   │   ├── clipboard.rs       # CF_UNICODETEXT clipboard
│   │   │   ├── events.rs          # WM_* → W3C event conversion
│   │   │   └── display.rs         # EnumDisplayMonitors
│   │   ├── macos/                 # macOS (AppKit/Cocoa)
│   │   │   ├── mod.rs
│   │   │   ├── platform.rs        # MacOSPlatform implementation
│   │   │   ├── window.rs          # NSWindow management
│   │   │   ├── clipboard.rs       # NSPasteboard
│   │   │   ├── events.rs          # NSEvent → W3C conversion
│   │   │   └── display.rs         # NSScreen enumeration
│   │   ├── headless/              # Testing/CI implementation
│   │   │   ├── mod.rs
│   │   │   └── platform.rs        # HeadlessPlatform (mock)
│   │   └── winit/                 # Optional Winit backend (future)
│   │       └── platform.rs        # WinitPlatform wrapper
│   ├── shared/                     # Shared infrastructure
│   │   ├── mod.rs
│   │   └── handlers.rs            # PlatformHandlers callback registry
│   ├── executor.rs                 # BackgroundExecutor, ForegroundExecutor
│   ├── config.rs                   # WindowOptions, WindowConfiguration
│   └── window.rs                   # WindowId, WindowMode, WindowEvent types
├── tests/                          # Integration tests
│   ├── contract.rs                # Platform trait compliance tests (T009)
│   └── integration_template.rs    # Cross-crate integration tests (T010)
├── examples/                       # Usage examples
│   ├── minimal_window.rs          # Basic platform test with tracing
│   └── simple_window.rs           # Window creation demo with tracing
├── benches/                        # Performance benchmarks (criterion)
│   ├── text_measurement.rs       # T116: <1ms text measurement
│   ├── event_dispatch.rs         # T117: <5ms event latency
│   ├── clipboard.rs              # T118: <1ms clipboard roundtrip
│   ├── display_enum.rs           # T119: <10ms display enumeration
│   └── executor_spawn.rs         # T120: <100µs spawn overhead
└── Cargo.toml                      # Dependencies, features, metadata
```

**Structure Decision**: Single library crate within FLUI monorepo workspace. Platform implementations organized by OS (windows/, macos/, headless/) with shared traits and infrastructure. Testing organized by type: unit tests inline with code (`#[cfg(test)]`), integration tests in `tests/` directory, contract tests for trait compliance, benchmarks in `benches/` for performance verification.

**Key Design Choices**:
- **Platform-specific modules**: Conditional compilation via `#[cfg(target_os = "...")]` enables OS-specific implementations
- **Trait-based abstraction**: Platform trait provides uniform interface across all implementations
- **Type erasure**: `Arc<dyn Platform>` enables platform-agnostic code without generics
- **Callback registry**: PlatformHandlers decouples framework from platform (GPUI-inspired pattern)
- **Headless testing**: Mock platform enables CI/CD without GPU or display server (FLUI_HEADLESS=1)

## Platform Text System Architecture

**Critical for MVP**: Text measurement and glyph shaping are blocking requirements for flui_painting integration.

### PlatformTextSystem Trait

```rust
use flui_types::{Rect, Pixels};
use std::sync::Arc;

/// Platform-native text measurement and glyph shaping abstraction.
/// 
/// Implementations:
/// - Windows: DirectWrite (IDWriteFactory, IDWriteTextLayout)
/// - macOS: Core Text (CTFont, CTLine, CTRun)
/// - Headless: Mock (returns fixed metrics for testing)
pub trait PlatformTextSystem: Send + Sync {
    /// Returns the default system font family name.
    /// 
    /// Platform defaults:
    /// - Windows: "Segoe UI"
    /// - macOS: "SF Pro Text"
    /// - Headless: "MockFont"
    fn default_font_family(&self) -> Result<String, TextSystemError>;
    
    /// Loads a font by family name and size in logical pixels.
    /// 
    /// Font fallback chain:
    /// 1. Requested family (if available)
    /// 2. System default font (platform-specific)
    /// 3. First available font (guaranteed fallback)
    /// 
    /// Never returns error - always falls back to available font.
    fn load_font(&self, family: &str, size: Pixels) -> FontHandle;
    
    /// Measures text bounding box in logical pixels.
    /// 
    /// Returns rect with:
    /// - origin: (0, 0) at baseline left
    /// - width: advance width including trailing whitespace
    /// - height: ascent + descent (line height)
    /// 
    /// Performance: <1ms for strings <100 ASCII characters (NFR-001)
    fn measure_text(&self, text: &str, font: &FontHandle) -> Rect<Pixels>;
    
    /// Shapes text into positioned glyphs for rendering.
    /// 
    /// Returns glyph positions relative to baseline origin (0, 0).
    /// Handles complex scripts (ligatures, diacritics, RTL text).
    /// 
    /// Performance: <1ms for strings <100 ASCII characters (NFR-001)
    fn shape_glyphs(&self, text: &str, font: &FontHandle) -> Vec<GlyphPosition>;
    
    /// Enumerates all available font families on the system.
    /// 
    /// Used for font pickers and preference UIs.
    fn enumerate_fonts(&self) -> Vec<String>;
}

/// Opaque handle to a loaded font (platform-specific).
/// 
/// Platform representations:
/// - Windows: IDWriteTextFormat (COM object)
/// - macOS: CTFont (CoreFoundation ref)
/// - Headless: Mock struct with size/family
#[derive(Clone)]
pub struct FontHandle {
    inner: Arc<dyn std::any::Any + Send + Sync>,
    family: String,
    size: Pixels,
}

/// Positioned glyph for rendering (output of shape_glyphs).
/// 
/// Coordinates in logical pixels relative to baseline origin.
/// Compatible with flui_painting::Canvas::draw_glyphs().
#[derive(Debug, Clone, Copy)]
pub struct GlyphPosition {
    /// Platform-specific glyph ID (index into font's glyph table)
    pub glyph_id: u32,
    
    /// Horizontal offset from baseline origin (logical pixels)
    pub x_offset: Pixels,
    
    /// Vertical offset from baseline (positive = above, negative = below)
    pub y_offset: Pixels,
    
    /// Horizontal advance to next glyph position (logical pixels)
    pub x_advance: Pixels,
}

#[derive(Debug, thiserror::Error)]
pub enum TextSystemError {
    #[error("Text system not initialized")]
    NotInitialized,
    
    #[error("Platform text API failed: {0}")]
    PlatformError(String),
}
```

### Windows DirectWrite Integration

**Dependencies**: `windows` crate 0.59+ with features: `Win32_Graphics_DirectWrite`, `Win32_Graphics_Direct2D`

**Initialization**:
```rust
use windows::Win32::Graphics::DirectWrite::*;

pub struct WindowsTextSystem {
    factory: IDWriteFactory,
    // Cache for loaded fonts (family + size -> IDWriteTextFormat)
    font_cache: DashMap<(String, Pixels), IDWriteTextFormat>,
}

impl WindowsTextSystem {
    pub fn new() -> Result<Self> {
        // Create DWrite factory (single-threaded COM, apartment-threaded)
        let factory: IDWriteFactory = unsafe {
            DWriteCreateFactory(
                DWRITE_FACTORY_TYPE_SHARED,
                &IDWriteFactory::IID,
            )?
        };
        
        Ok(Self {
            factory,
            font_cache: DashMap::new(),
        })
    }
}
```

**Text Measurement**:
- Use `IDWriteTextLayout` for accurate bounding box (handles multi-line, complex scripts)
- Convert DWRITE_TEXT_METRICS to Rect<Pixels>
- Cache layout objects for repeated measurements (clear on font change)

**Glyph Shaping**:
- Use `IDWriteTextLayout::GetGlyphRuns()` to extract positioned glyphs
- Convert DWRITE_GLYPH_RUN to Vec<GlyphPosition>
- Handle Unicode normalization (DirectWrite does this automatically)

**Unicode Support**:
- DirectWrite handles full Unicode 15.0 including:
  - Emoji sequences (ZWJ, variation selectors)
  - Complex scripts (Devanagari, Arabic, Thai)
  - Bidirectional text (automatic bidi algorithm)
  - Font fallback (automatic for missing glyphs)

### macOS Core Text Integration

**Dependencies**: `cocoa` crate 0.26+, `core-text` crate 0.24+, `core-foundation` crate 0.10+

**Initialization**:
```rust
use core_text::{font::CTFont, line::CTLine, run::CTRun};
use core_foundation::string::CFString;

pub struct MacOSTextSystem {
    // No global state needed - Core Text is pure functional API
    // Cache for loaded fonts (family + size -> CTFont)
    font_cache: DashMap<(String, Pixels), CTFont>,
}

impl MacOSTextSystem {
    pub fn new() -> Self {
        Self {
            font_cache: DashMap::new(),
        }
    }
}
```

**Text Measurement**:
- Use `CTLine::create_with_attributed_string()` for text layout
- Get bounds via `CTLine::get_typographic_bounds()` and `CTLine::get_image_bounds()`
- Convert to Rect<Pixels> (Core Text uses CGFloat, same as logical pixels on macOS)

**Glyph Shaping**:
- Use `CTLine::glyph_runs()` to get CTRun array
- Extract glyphs via `CTRun::glyphs()` and positions via `CTRun::positions()`
- Convert CGPoint positions to GlyphPosition structs

**Unicode Support**:
- Core Text handles full Unicode 15.0 including:
  - Emoji sequences (automatic with Apple Color Emoji font)
  - Complex scripts (built-in for all system languages)
  - Bidirectional text (NSAttributedString + CTLine handles bidi)
  - Font cascade (automatic fallback via CTFont cascade list)

### Headless Mock Implementation

**Purpose**: Enable CI/CD testing without platform text APIs.

```rust
pub struct HeadlessTextSystem {
    // Fixed metrics for deterministic testing
}

impl PlatformTextSystem for HeadlessTextSystem {
    fn measure_text(&self, text: &str, font: &FontHandle) -> Rect<Pixels> {
        // Mock: 10px per character, 16px line height
        let char_count = text.chars().count();
        Rect::new(0.0, 0.0, char_count as f32 * 10.0, font.size)
    }
    
    fn shape_glyphs(&self, text: &str, font: &FontHandle) -> Vec<GlyphPosition> {
        // Mock: simple linear glyph positions (no ligatures/diacritics)
        text.chars()
            .enumerate()
            .map(|(i, ch)| GlyphPosition {
                glyph_id: ch as u32,
                x_offset: Pixels(i as f32 * 10.0),
                y_offset: Pixels(0.0),
                x_advance: Pixels(10.0),
            })
            .collect()
    }
}
```

### Integration with flui_painting

**Contract**: `Canvas::draw_glyphs()` accepts Vec<GlyphPosition> from PlatformTextSystem.

```rust
// In flui_painting crate:
impl Canvas {
    /// Draws shaped glyphs using positions from PlatformTextSystem.
    /// 
    /// font_handle: Opaque font from text system (contains platform font)
    /// glyphs: Pre-shaped glyph positions (from shape_glyphs)
    /// baseline_origin: Starting point in canvas coordinates
    /// paint: Fill/stroke style for glyph rendering
    pub fn draw_glyphs(
        &mut self,
        font_handle: &FontHandle,
        glyphs: &[GlyphPosition],
        baseline_origin: Point<Pixels>,
        paint: &Paint,
    ) {
        // Extract platform font from handle
        // Render glyphs to wgpu texture atlas
        // Generate draw commands for GPU
    }
}
```

**Data Flow**:
1. Application calls `text_system.shape_glyphs(text, font)` → Vec<GlyphPosition>
2. Application calls `canvas.draw_glyphs(font, glyphs, origin, paint)`
3. Canvas extracts platform font from FontHandle
4. Canvas renders glyphs to texture atlas (glyphon or custom renderer)
5. Canvas generates GPU draw commands with glyph positions

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

**No Constitution Violations**: All principles satisfied (see Constitution Check section above). No complexity exceptions needed.
