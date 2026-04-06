// Signed Distance Field (SDF) Utility Library for FLUI
//
// This library provides reusable SDF functions for rendering 2D primitives
// with perfect antialiasing and efficient GPU execution.
//
// SDF advantages:
// - Branchless execution (no if/else in fragment shader)
// - Adaptive antialiasing via fwidth()
// - CSG operations (union, subtraction, intersection)
// - Resolution-independent rendering
//
// Reference: Inigo Quilez - https://iquilezles.org/articles/distfunctions2d/

// =============================================================================
// Basic 2D Shapes
// =============================================================================

/// Circle signed distance field
/// Returns: distance to circle surface (negative inside, positive outside)
fn sdCircle(p: vec2<f32>, r: f32) -> f32 {
    return length(p) - r;
}

/// Box (rectangle) signed distance field with sharp corners
/// p: point to test
/// b: half-extents (half width, half height)
fn sdBox(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

/// Rounded box (rectangle) with per-corner radii
/// p: point to test (centered at origin)
/// b: half-extents (half width, half height)
/// r: corner radii [top-left, top-right, bottom-right, bottom-left]
///
/// This is the core function for UI rectangles with rounded corners.
/// Branchless implementation using select() for optimal GPU performance.
fn sdRoundedBox(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // Select radius based on quadrant (branchless!)
    // Quadrant determination:
    // - p.x > 0.0: right side (uses r.y or r.z)
    // - p.x < 0.0: left side (uses r.x or r.w)
    // - p.y > 0.0: bottom (uses r.z or r.w)
    // - p.y < 0.0: top (uses r.x or r.y)
    let r2 = select(r.zw, r.xy, p.x > 0.0);  // Select left/right pair
    let r3 = select(r2.y, r2.x, p.y > 0.0);  // Select top/bottom from pair

    let q = abs(p) - b + vec2<f32>(r3);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r3;
}

/// Oriented box (rectangle with rotation)
/// p: point to test
/// b: half-extents
/// th: rotation angle in radians
fn sdOrientedBox(p: vec2<f32>, b: vec2<f32>, th: f32) -> f32 {
    let c = cos(th);
    let s = sin(th);
    let rot = mat2x2<f32>(c, -s, s, c);
    let rotated_p = rot * p;
    return sdBox(rotated_p, b);
}

/// Ellipse signed distance field
/// p: point to test
/// ab: semi-axes (half width, half height)
fn sdEllipse(p: vec2<f32>, ab: vec2<f32>) -> f32 {
    let p2 = abs(p);

    // Polynomial approximation (accurate for UI shapes)
    if (p2.x > p2.y) {
        let q = p2.xy / ab;
        let r = (q.x * q.x + q.y * q.y - 1.0) / length(q);
        return r * min(ab.x, ab.y);
    } else {
        let q = p2.yx / ab.yx;
        let r = (q.x * q.x + q.y * q.y - 1.0) / length(q);
        return r * min(ab.x, ab.y);
    }
}

// =============================================================================
// Antialiasing
// =============================================================================

/// Convert SDF distance to alpha value with adaptive antialiasing
/// Uses screen-space derivatives (fwidth) for resolution-independent AA
///
/// dist: signed distance from SDF function
/// Returns: alpha value [0.0, 1.0] for blending
fn sdfToAlpha(dist: f32) -> f32 {
    // fwidth(dist) = abs(dFdx(dist)) + abs(dFdy(dist))
    // This gives us the rate of change across the pixel, allowing
    // adaptive antialiasing that works at any zoom level
    let edge_width = fwidth(dist) * 0.5;

    // smoothstep from -edge to +edge creates smooth transition
    return 1.0 - smoothstep(-edge_width, edge_width, dist);
}

/// Manual antialiasing for cases where fwidth() is not available
/// or when you want explicit control over AA width
///
/// dist: signed distance
/// aa_width: antialiasing width in pixels (typically 0.5 - 1.0)
fn sdfToAlphaManual(dist: f32, aa_width: f32) -> f32 {
    return 1.0 - smoothstep(-aa_width, aa_width, dist);
}

// =============================================================================
// CSG (Constructive Solid Geometry) Operations
// =============================================================================

/// Union (OR) - combines two shapes
fn sdUnion(d1: f32, d2: f32) -> f32 {
    return min(d1, d2);
}

/// Subtraction - carve d1 out of d2
fn sdSubtraction(d1: f32, d2: f32) -> f32 {
    return max(-d1, d2);
}

/// Intersection (AND) - only where both shapes overlap
fn sdIntersection(d1: f32, d2: f32) -> f32 {
    return max(d1, d2);
}

/// Smooth union (rounded blend between shapes)
/// k: smoothing factor (typically 0.1 - 0.5)
fn sdSmoothUnion(d1: f32, d2: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (d2 - d1) / k, 0.0, 1.0);
    return mix(d2, d1, h) - k * h * (1.0 - h);
}

/// Smooth subtraction
fn sdSmoothSubtraction(d1: f32, d2: f32, k: f32) -> f32 {
    let h = clamp(0.5 - 0.5 * (d2 + d1) / k, 0.0, 1.0);
    return mix(d2, -d1, h) + k * h * (1.0 - h);
}

/// Smooth intersection
fn sdSmoothIntersection(d1: f32, d2: f32, k: f32) -> f32 {
    let h = clamp(0.5 - 0.5 * (d2 - d1) / k, 0.0, 1.0);
    return mix(d2, d1, h) + k * h * (1.0 - h);
}

// =============================================================================
// Domain Operations (transformations)
// =============================================================================

/// Repeat domain in 2D grid (for patterns, tiles)
/// p: point to test
/// spacing: distance between repetitions
fn opRepeat(p: vec2<f32>, spacing: vec2<f32>) -> vec2<f32> {
    return (p % spacing) - spacing * 0.5;
}

/// Polar repetition (for radial patterns)
/// p: point to test
/// n: number of repetitions
fn opPolarRepeat(p: vec2<f32>, n: f32) -> vec2<f32> {
    let angle = 2.0 * 3.14159265359 / n;
    let a = atan2(p.y, p.x) + angle * 0.5;
    let r = length(p);
    let c = floor(a / angle);
    let a2 = (a % angle) - angle * 0.5;
    return vec2<f32>(cos(a2), sin(a2)) * r;
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Convert from UV coordinates [0,1] to centered coordinates
/// Useful for applying SDFs in fragment shaders
fn uvToCentered(uv: vec2<f32>, size: vec2<f32>) -> vec2<f32> {
    return (uv - 0.5) * size;
}

/// Aspect ratio correction for non-square shapes
fn correctAspect(p: vec2<f32>, aspect: f32) -> vec2<f32> {
    return vec2<f32>(p.x * aspect, p.y);
}
