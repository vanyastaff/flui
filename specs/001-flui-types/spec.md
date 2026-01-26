# Feature Specification: flui-types Crate

**Feature Branch**: `001-flui-types`
**Created**: 2026-01-26
**Status**: Draft

## Feature Overview

The flui-types crate provides fundamental measurement and geometry tools for the Flui framework. Its primary job is to **prevent bugs caused by mixing incompatible units** (like accidentally adding screen pixels to layout pixels).

**The Problem We're Solving:**
In cross-platform UI development, the #1 source of rendering bugs is mixing different types of measurements:
- Layout calculations use device-independent pixels (DPI-independent)
- Rendering uses actual screen pixels (varies by device)
- Typography uses font-relative units (for accessibility)

When developers accidentally mix these, buttons appear at wrong sizes, layouts break on high-DPI screens, and accessibility features fail.

**Our Solution:**
Make measurement units part of the type system. The compiler prevents mixing incompatible units, forcing developers to explicitly convert when needed. This catches bugs at compile time instead of runtime.

**Business Value:**
- Fewer cross-platform rendering bugs (saves QA time)
- Clearer code (self-documenting measurements)
- Better accessibility (proper support for user font size preferences)
- Faster development (compiler catches mistakes immediately)

---

## User Scenarios & Testing

### User Story 1 - Device-Independent Layout (Priority: P1)

**As a** widget developer
**I want to** specify widget sizes in device-independent units
**So that** my widgets look the same size on all devices (1x, 2x, 3x displays)

**Why this priority**: This is the foundation of the entire type system. Without device-independent layout, the framework cannot support cross-platform rendering at all. This is the minimum viable product.

**Independent Test**: Can be fully tested by creating a simple rectangle widget with logical pixel dimensions, running it on displays with different scale factors, and measuring the visual size remains constant.

**Acceptance Scenarios**:

1. **Given** a button widget specified as 100 logical pixels wide, **When** rendered on a 1x display, **Then** it appears as 100 screen pixels
2. **Given** the same button widget, **When** rendered on a 2x display (Retina), **Then** it appears as 200 screen pixels
3. **Given** the same button widget, **When** rendered on a 3x display, **Then** it appears as 300 screen pixels
4. **Given** widget layout code, **When** developer specifies dimensions, **Then** they never need to manually calculate scale factor

---

### User Story 2 - Prevent Unit Mixing Bugs (Priority: P1)

**As a** developer
**I want to** get compile errors when mixing incompatible units
**So that** I can't accidentally create rendering bugs

**Why this priority**: This is the core value proposition of the type system. Without compile-time safety, the system is no better than plain floats. This is essential for the MVP.

**Independent Test**: Can be fully tested by writing test cases that attempt to mix incompatible units and verifying the compiler rejects them with clear error messages.

**Acceptance Scenarios**:

1. **Given** a rectangle in logical pixels, **When** developer tries to add an offset in device pixels, **Then** compiler produces error "Cannot add Rect<Pixels> and Offset<DevicePixels>"
2. **Given** a function expecting logical pixels, **When** developer passes device pixels, **Then** compiler rejects the call with type mismatch error
3. **Given** a compiler error about unit mixing, **When** developer reads the error, **Then** error message explains the mismatch and suggests conversions
4. **Given** incompatible unit types, **When** developer wants to combine them, **Then** they must explicitly convert with scale factor specified

---

### User Story 3 - Geometric Calculations (Priority: P1)

**As a** layout algorithm developer
**I want to** perform geometric calculations clearly
**So that** my code is readable and maintainable

**Why this priority**: Basic geometric operations (intersection, containment, distances) are required by every layout algorithm. Without these, the type system cannot support any real UI work. Critical for MVP.

**Independent Test**: Can be fully tested by writing layout algorithms that use geometric operations (hit testing, bounds checking, etc.) and verifying results are correct and code is readable.

**Acceptance Scenarios**:

1. **Given** a button's rectangular bounds and a tap position, **When** checking if tap is inside button, **Then** code reads `button_rect.contains(tap_position)`
2. **Given** two overlapping rectangles, **When** calculating their intersection, **Then** code reads `rect1.intersect(rect2)` and returns overlapping area
3. **Given** a rectangle, **When** expanding by 10 pixels on all sides, **Then** code reads `rect.inflate(10)` and returns expanded rectangle
4. **Given** two points, **When** calculating distance, **Then** code reads `point1.distance_to(point2)` and returns correct value

---

### User Story 4 - Font-Relative Sizing (Priority: P2)

**As a** developer implementing accessible UIs
**I want to** specify spacing relative to font size
**So that** layouts scale when users increase font size for readability

**Why this priority**: Essential for accessibility compliance and inclusive design. Not required for basic rendering, but critical for production apps. Important for Phase 2.

**Independent Test**: Can be fully tested by creating a layout with rem-based spacing, changing the base font size, and verifying all spacing scales proportionally.

**Acceptance Scenarios**:

1. **Given** padding specified as 2 rems and base font size of 16px, **When** layout is calculated, **Then** padding is 32px
2. **Given** the same padding, **When** user increases font to 20px, **Then** padding automatically becomes 40px
3. **Given** a complete UI layout with rem-based spacing, **When** base font size changes, **Then** entire UI scales proportionally
4. **Given** rem units, **When** developer uses them, **Then** behavior matches CSS rem units (familiar to web developers)

---

### User Story 5 - Explicit Unit Conversions (Priority: P2)

**As a** rendering engineer
**I want to** explicitly convert between unit types
**So that** the code is self-documenting and I understand what's happening

**Why this priority**: Required for the boundary between layout and rendering. Conversion must be explicit to maintain type safety. Important for Phase 2.

**Independent Test**: Can be fully tested by writing code that converts between logical and device pixels in both directions and verifying conversions are correct and explicit.

**Acceptance Scenarios**:

1. **Given** a layout rectangle in logical pixels, **When** converting to device pixels for GPU rendering, **Then** developer writes `layout_rect.to_device_pixels(scale_factor: 2.0)`
2. **Given** a touch position in device pixels, **When** converting to logical pixels for hit testing, **Then** developer writes `touch_pos.to_logical_pixels(scale_factor: 2.0)`
3. **Given** any unit conversion, **When** reading the code, **Then** the conversion is self-documenting with explicit scale factor
4. **Given** a conversion method call, **When** inspecting the code, **Then** method name makes conversion direction obvious

---

### User Story 6 - Padding and Margins (Priority: P2)

**As a** widget developer
**I want to** specify padding/margins flexibly
**So that** I can match design specs exactly

**Why this priority**: Common UI pattern needed by almost all widgets. Makes the API ergonomic and practical. Important for Phase 2.

**Independent Test**: Can be fully tested by creating widgets with various padding configurations and verifying they match design specs exactly.

**Acceptance Scenarios**:

1. **Given** a button with uniform padding, **When** developer specifies it, **Then** code reads `padding: all(12)` (same on all sides)
2. **Given** a button with different vertical/horizontal padding, **When** developer specifies it, **Then** code reads `padding: vertical(12), horizontal(24)`
3. **Given** a button with individual side padding, **When** developer specifies it, **Then** code reads `padding: top(8), right(16), bottom(8), left(16)`
4. **Given** padding specification, **When** applied to rectangle, **Then** system handles shrinking rectangle by insets

---

### User Story 7 - Working with Colors (Priority: P2)

**As a** UI developer
**I want to** use colors from design specs directly
**So that** I don't have to manually convert between formats

**Why this priority**: Required for any visual UI. Color operations (opacity, blending) are common patterns. Important for Phase 2.

**Independent Test**: Can be fully tested by creating UI elements with various color specifications and verifying they render correctly.

**Acceptance Scenarios**:

1. **Given** a brand color hex code "#FF5733" from design tools, **When** developer uses it, **Then** color renders exactly as specified
2. **Given** a color, **When** creating hover state, **Then** developer writes `color.with_opacity(0.8)` to adjust transparency
3. **Given** two colors, **When** blending them, **Then** developer writes `color1.mix(color2, ratio: 0.5)` for equal blend
4. **Given** a color, **When** converting to HSL for adjustments, **Then** developer can lighten/darken easily

---

### User Story 8 - Precise Rendering (Priority: P3)

**As a** graphics engineer
**I want to** control exact screen pixels when needed
**So that** I can render hairlines and pixel-perfect graphics

**Why this priority**: Advanced feature for specialized rendering. Not required for basic UI work. Nice-to-have for Phase 3.

**Independent Test**: Can be fully tested by drawing 1-pixel hairlines on various displays and verifying they remain crisp without blurring.

**Acceptance Scenarios**:

1. **Given** a 1-pixel border requirement, **When** developer uses device pixels explicitly, **Then** border is 1 screen pixel on 1x display
2. **Given** the same border, **When** rendered on 2x display, **Then** border is 2 screen pixels (same visual thickness)
3. **Given** coordinates that need pixel alignment, **When** developer rounds them, **Then** graphics remain crisp without anti-aliasing blur
4. **Given** device pixel usage, **When** reading code, **Then** it's obvious when working in device vs logical pixels

---

### User Story 9 - Corner Radii (Priority: P3)

**As a** widget developer
**I want to** specify corner radii for rounded rectangles
**So that** I can create modern UI designs

**Why this priority**: Common modern UI pattern but not essential for MVP. Can be added later. Nice-to-have for Phase 3.

**Independent Test**: Can be fully tested by creating widgets with rounded corners and verifying they render correctly with various radius configurations.

**Acceptance Scenarios**:

1. **Given** a card with uniform corner radius, **When** developer specifies it, **Then** code reads `corners: all(8)` (8px radius on all corners)
2. **Given** a card with rounded top corners only, **When** developer specifies it, **Then** code reads `corners: top(8), bottom(0)`
3. **Given** a card with individual corner radii, **When** developer specifies it, **Then** each corner can have different radius
4. **Given** rounded corner specification, **When** rendering, **Then** system handles clipping content to rounded shape

---

### User Story 10 - Layout Direction Support (Priority: P3)

**As a** internationalization developer
**I want to** specify measurements that work in RTL languages
**So that** UI automatically mirrors for Arabic, Hebrew, etc.

**Why this priority**: Important for global apps but not required for initial MVP. Can be added in later phase. Nice-to-have for Phase 3.

**Independent Test**: Can be fully tested by creating a layout with start/end-based spacing, switching to RTL mode, and verifying the layout mirrors correctly.

**Acceptance Scenarios**:

1. **Given** padding using "start/end" instead of "left/right", **When** layout direction is LTR, **Then** start=left, end=right
2. **Given** the same padding, **When** layout direction is RTL, **Then** start=right, end=left (automatically mirrored)
3. **Given** a complete UI layout, **When** switching to RTL language, **Then** entire layout mirrors without code changes
4. **Given** coordinate system, **When** working with layout direction, **Then** system is aware of directionality

---

### Edge Cases

- **Invalid Geometry**: Rectangle with negative width/height → normalize to valid rectangle (width/height clamped to 0)
- **Empty Rectangles**: Rectangle with zero size → clearly identifiable via `is_empty()` method, doesn't crash
- **Points at Infinity/NaN**: Points with infinite or NaN coordinates → panic in debug build with clear message, clamp to valid range in release
- **Out-of-Range Colors**: RGB values > 255 or < 0 → clamp to valid range [0, 255]
- **Alpha Out of Range**: Alpha > 1.0 or < 0.0 → clamp to valid range [0.0, 1.0]
- **Invalid Hex Codes**: Malformed hex color strings → panic in debug with clear message, fall back to default color in release
- **Division by Zero**: Rectangle scaling by zero → results in zero-sized geometry (doesn't panic)
- **Coincident Points**: Distance calculation between same point → returns 0.0 correctly

---

## Requirements

### Functional Requirements

**Unit Type System:**

- **FR-001**: System MUST prevent mixing incompatible unit types at compile time (e.g., cannot add Pixels + DevicePixels)
- **FR-002**: System MUST support Logical Pixels (device-independent) for layout calculations
- **FR-003**: System MUST support Device Pixels (screen pixels) for GPU rendering
- **FR-004**: System MUST support Rems (font-relative) for typography-based spacing
- **FR-005**: System MUST support Scaled Pixels (internal) for pre-scaling calculations
- **FR-006**: System MUST require explicit conversions between unit types with scale factor/context specified
- **FR-007**: Unit conversions MUST be self-documenting (method names make conversion direction obvious)

**Geometric Primitives:**

- **FR-008**: System MUST represent 2D points with x, y coordinates
- **FR-009**: System MUST calculate distances between points
- **FR-010**: System MUST represent sizes (width, height) for rectangles
- **FR-011**: System MUST check if size is empty (zero or negative dimensions)
- **FR-012**: System MUST represent axis-aligned rectangles
- **FR-013**: System MUST support creating rectangles from: origin+size, left/top/width/height, two corners
- **FR-014**: System MUST query rectangle properties: left, top, right, bottom, width, height, center
- **FR-015**: System MUST check if rectangle contains a point
- **FR-016**: System MUST check if two rectangles overlap
- **FR-017**: System MUST calculate rectangle intersection (overlapping area)
- **FR-018**: System MUST calculate rectangle union (bounding box)
- **FR-019**: System MUST inflate/deflate rectangles (expand/shrink)
- **FR-020**: System MUST offset rectangles (move position)
- **FR-021**: System MUST apply insets to rectangles (padding/margin)

**Edges and Corners:**

- **FR-022**: System MUST represent four-sided values (top, right, bottom, left) for padding/margins
- **FR-023**: System MUST support specifying same value for all sides
- **FR-024**: System MUST support symmetric values (vertical/horizontal)
- **FR-025**: System MUST support individual values per side
- **FR-026**: System MUST represent per-corner values for radii
- **FR-027**: System MUST support same radius for all corners
- **FR-028**: System MUST support individual radius per corner

**Color System:**

- **FR-029**: System MUST create colors from hex codes ("#FF5733", "#FF5733AA")
- **FR-030**: System MUST create colors from RGB values (red, green, blue 0-255)
- **FR-031**: System MUST create colors from RGBA values (RGB + alpha)
- **FR-032**: System MUST provide named colors (red, blue, green, black, white, transparent, grays)
- **FR-033**: System MUST support HSL color format (hue, saturation, lightness)
- **FR-034**: System MUST adjust color opacity
- **FR-035**: System MUST blend two colors
- **FR-036**: System MUST lighten/darken colors
- **FR-037**: System MUST convert between RGB and HSL formats
- **FR-038**: System MUST extract color components (red, green, blue, alpha values)

**Developer Experience:**

- **FR-039**: System SHOULD provide concise helper functions (e.g., `px(10)` instead of `Pixels(10.0)`)
- **FR-040**: System MUST provide clear compiler error messages that explain unit mismatches
- **FR-041**: Error messages MUST suggest how to fix unit type errors
- **FR-042**: System SHOULD work well with IDEs (autocomplete, type hints, hover docs)

**Performance:**

- **FR-043**: Point type MUST be ≤ 8 bytes in memory
- **FR-044**: Size type MUST be ≤ 8 bytes in memory
- **FR-045**: Rectangle type MUST be ≤ 20 bytes in memory
- **FR-046**: Color type MUST be ≤ 16 bytes in memory
- **FR-047**: All types MUST be stack-allocated (no heap allocations)
- **FR-048**: Point distance calculation MUST complete in < 10 nanoseconds
- **FR-049**: Rectangle intersection MUST complete in < 20 nanoseconds
- **FR-050**: Color blending MUST complete in < 20 nanoseconds
- **FR-051**: Unit conversions MUST optimize to near-zero cost (ideally optimized away by compiler)
- **FR-052**: Crate MUST compile in < 5 seconds (clean build)

**Edge Cases:**

- **FR-053**: System MUST handle negative rectangle dimensions by normalizing to valid rectangle
- **FR-054**: System MUST clearly identify empty rectangles without crashing
- **FR-055**: System MUST handle points at infinity/NaN by panicking in debug, clamping in release
- **FR-056**: System MUST clamp RGB values to valid range [0, 255]
- **FR-057**: System MUST clamp alpha values to valid range [0.0, 1.0]
- **FR-058**: System MUST handle invalid hex codes by panicking in debug with clear message
- **FR-059**: System MUST handle division by zero in geometric calculations without panicking

### Key Entities

- **Unit (concept)**: A type-safe wrapper around numeric measurements that prevents mixing incompatible unit types. Cannot be mixed accidentally, can be explicitly converted, optimizes to plain numbers at runtime (zero cost). Examples: Pixels, DevicePixels, Rems, ScaledPixels.

- **Point**: A 2D coordinate in space with x and y coordinates. Each coordinate has a unit type. Used for positions, not sizes or offsets.

- **Size**: 2D dimensions (width and height). Each dimension has a unit type. Width and height must be non-negative. Used for widget dimensions, constraints.

- **Rectangle**: An axis-aligned rectangular region defined by origin point and size. Can be queried as left/top/right/bottom. All measurements have same unit type. Used for widget bounds, hit testing, clipping, layout constraints.

- **Color**: An RGBA color value with red, green, blue, alpha components. Can be created from hex codes or RGB values. Supports blending and opacity adjustments. Can convert between RGB and HSL.

- **Edges/Insets**: Four values representing top, right, bottom, left measurements. All four have same unit type. Used for padding, margins, safe areas. Can specify all same, symmetric, or individual values.

- **Corners**: Four values representing measurements for each corner (top-left, top-right, bottom-right, bottom-left). Generic over value type (typically used for radii). Can specify all same or individual values.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: Widget developer can specify button as "100 logical pixels wide" and it appears same visual size across 1x, 2x, and 3x displays
- **SC-002**: Attempting to mix Pixels + DevicePixels produces compile error with clear explanation and suggested fixes
- **SC-003**: Layout code that checks "point inside rectangle" reads as `rect.contains(point)` without complex math
- **SC-004**: Converting layout rect to device pixels reads as `rect.to_device_pixels(scale_factor)` - self-documenting
- **SC-005**: UI with rem-based spacing scales proportionally when user changes base font size from 16px to 24px
- **SC-006**: Developer can specify padding as `vertical(12), horizontal(24)` matching design spec exactly
- **SC-007**: Brand color "#FF5733" from design tools can be used directly in code without manual conversion
- **SC-008**: 1-pixel hairline border remains crisp without blur on both 1x and 2x displays
- **SC-009**: Point distance calculation completes in under 10 nanoseconds (measured via benchmarks)
- **SC-010**: Rectangle intersection calculation completes in under 20 nanoseconds (measured via benchmarks)
- **SC-011**: Color blending operation completes in under 20 nanoseconds (measured via benchmarks)
- **SC-012**: Crate compiles from scratch in under 5 seconds (measured on standard dev machine)
- **SC-013**: No heap allocations occur during basic geometric operations (verified via profiling)
- **SC-014**: Point type is exactly 8 bytes (verified via `std::mem::size_of`)
- **SC-015**: Rectangle type is 16-20 bytes (verified via `std::mem::size_of`)
- **SC-016**: 90%+ code coverage from tests (measured via coverage tools)
- **SC-017**: Property-based tests verify geometric invariants (e.g., intersection is commutative)
- **SC-018**: All public APIs have documentation with examples (verified via `cargo doc --document-private-items`)
- **SC-019**: Simple widget code can be written without consulting documentation (validated via user testing)
- **SC-020**: IDE provides autocomplete and type hints for all unit types (verified in VS Code, RustRover)

---

## Assumptions

### Coordinate System
- Origin (0, 0) is at top-left (standard for UI frameworks)
- X increases rightward
- Y increases downward
- Rotations are clockwise-positive (standard for UI)

### Floating Point Precision
- Using 32-bit floats (f32) is sufficient for UI work
- Accept minor precision loss for better performance and memory
- Equality comparisons allow small tolerance (epsilon)

### Platform Independence
- This crate has no platform-specific code
- Works identically on Windows, macOS, Linux, WASM
- No FFI boundaries in this crate

### Scale Factors
- Scale factor is always positive
- Common values: 1.0, 1.5, 2.0, 3.0, 4.0
- Fractional scale factors are supported (1.25, 1.75, etc.)

### Font Sizes
- Base font size is always positive
- Common default: 16 pixels
- User preferences may change base font size
- All font-relative measurements scale proportionally

### Color Space
- Default color space is sRGB (standard for web/UI)
- Linear RGB available for correct blending
- No HDR or wide gamut in v1 (future enhancement)

---

## Scope Limitations

### Out of Scope for V1

**3D Geometry**
- No 3D points, vectors, or transformations
- May add basic 3D matrix type as placeholder for future

**Advanced Color Features**
- No HDR (high dynamic range)
- No wide color gamut (Display P3, etc.)
- No color profiles or ICC support

**Bezier Curves and Paths**
- Path geometry belongs in separate crate (flui-painting)
- This crate only handles simple rectangles

**Text Layout**
- Text metrics and layout belong in text-specific crate
- This crate provides units and colors for text rendering

**Animations**
- Animation interpolation belongs in flui-animation crate
- This crate provides the types to animate between

**Complex Transformations**
- 2D affine transforms may be added in v1.1
- Perspective transforms out of scope

---

## Dependencies

### Internal Flui Dependencies
**NONE** - This is the foundation crate, depends on nothing else in Flui.

### External Dependencies
**Minimal** - Prefer standard library over external crates.

**Acceptable:**
- Math utilities (if not in std)
- SIMD optimizations (optional feature flag)
- Testing dependencies (proptest, criterion)

**Avoid:**
- Heavy dependencies that slow compilation
- Proc macros (except derive macros)
- Platform-specific dependencies
