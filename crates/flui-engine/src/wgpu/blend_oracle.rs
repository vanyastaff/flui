//! A CPU model of the fixed-function blender, shared by every readback suite
//! that asserts an exact blended byte.
//!
//! The model is built from [`blend_state_for`] — production's own mode table —
//! so a mode cannot be classified one way in a test and another way on the
//! device. What it deliberately does NOT reproduce is the coverage correction:
//! [`coverage_correct`] states the coverage-correct DEFINITION
//! (`mix(dst, blend_at_full_coverage, cov)`), so a fix that is consistently
//! wrong fails against it rather than agreeing with itself.
//!
//! It lives here rather than inside one suite because two now depend on it —
//! the tessellated shape path's and the gradient path's — and a second copy of
//! a blender is a second opinion about what `Modulate` means. The
//! anti-aliased-clip geometry both suites sample lives here for the same
//! reason: [`FRINGE_COVERAGE`] is derived on paper, and one derivation is
//! enough.

use flui_painting::BlendMode;
use flui_types::Color;

use super::pipeline::blend_state_for;

/// Every fixed-function mode [`blend_state_for`] maps, so a sweep over the
/// Porter-Duff set cannot quietly skip one.
pub(super) const PORTER_DUFF_MODES: [BlendMode; 14] = [
    BlendMode::Clear,
    BlendMode::Src,
    BlendMode::Dst,
    BlendMode::SrcOver,
    BlendMode::DstOver,
    BlendMode::SrcIn,
    BlendMode::DstIn,
    BlendMode::SrcOut,
    BlendMode::DstOut,
    BlendMode::SrcATop,
    BlendMode::DstATop,
    BlendMode::Xor,
    BlendMode::Plus,
    BlendMode::Modulate,
];

/// Byte tolerance per channel, absorbing `Rgba8Unorm` quantisation and the
/// difference between the GPU's `smoothstep` and this file's arithmetic.
pub(super) const TOLERANCE: i32 = 2;

// ── The anti-aliased clip both suites blend through ──────────────────────────

/// Side of the square surface every oracle renders.
pub(super) const SIDE: u32 = 64;

/// The clip's left edge, in device pixels.
///
/// Deliberately a quarter-pixel past a column boundary. The clip's bounding-box
/// scissor truncates (`state_stack::clip_rect`), so it starts at column 15 and
/// the feathered column at 15 survives it. The mirror-image choice on the RIGHT
/// edge does not: there the scissor ends at `floor(right)` and cuts the one
/// column the feather lives in — which is what `clip_rect`'s own comment means
/// by "the outer half of the feather is lost there".
pub(super) const CLIP_LEFT: f32 = 15.75;

/// Top of the clip, in device pixels.
pub(super) const CLIP_TOP: f32 = 8.0;
/// Width of the clip, in device pixels.
pub(super) const CLIP_WIDTH: f32 = 32.0;
/// Height of the clip, in device pixels.
///
/// 48 with [`CLIP_RADIUS`] 8 keeps both corner arcs inside rows 8..16 and
/// 48..56, clear of [`SAMPLE_ROW`].
pub(super) const CLIP_HEIGHT: f32 = 48.0;
/// Circular corner radius of the clip, in device pixels.
pub(super) const CLIP_RADIUS: f32 = 8.0;

/// Coverage `sdfToAlpha` reports at [`FRINGE_COLUMN`], derived rather than
/// measured.
///
/// On the clip's straight left edge the rounded-box SDF reduces to
/// `distance = CLIP_LEFT − x`, whose screen-space gradient magnitude is 1, so
/// `edge_width = 0.5`. At the column's pixel centre `x = 15.5` that is
/// `distance = 0.25`, and `1 − smoothstep(−0.5, 0.5, 0.25)` with
/// `t = 0.75` is `1 − 0.75²·(3 − 2·0.75) = 1 − 0.84375`.
pub(super) const FRINGE_COVERAGE: f32 = 0.15625;

/// The single column the clip's left edge partially covers.
pub(super) const FRINGE_COLUMN: u32 = 15;
/// A column well inside the clip, clear of both the edge and the corner arcs.
pub(super) const COVERED_COLUMN: u32 = 20;
/// A column outside the clip, and outside its bounding-box scissor.
pub(super) const UNCOVERED_COLUMN: u32 = 10;
/// The sampled row: the clip's vertical midline, where the corner arcs cannot
/// reach and the edge is exactly vertical.
pub(super) const SAMPLE_ROW: u32 = 32;

/// The three sampled pixels of one render through that clip.
#[derive(Debug, Clone, Copy)]
pub(super) struct EdgeSamples {
    /// [`UNCOVERED_COLUMN`]: the destination must be untouched here.
    pub(super) outside_the_clip: [u8; 4],
    /// [`FRINGE_COLUMN`]: coverage [`FRINGE_COVERAGE`], where the corrected and
    /// folded predictions part company.
    pub(super) partially_covered: [u8; 4],
    /// [`COVERED_COLUMN`]: coverage 1, where they must agree.
    pub(super) fully_covered: [u8; 4],
}

/// Read [`SAMPLE_ROW`]'s three sampled columns out of an `Rgba8` readback of a
/// [`SIDE`]×[`SIDE`] surface.
pub(super) fn sample_the_clip_edge(pixels: &[u8]) -> EdgeSamples {
    let at = |column: u32| {
        let index = ((SAMPLE_ROW * SIDE + column) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    };
    EdgeSamples {
        outside_the_clip: at(UNCOVERED_COLUMN),
        partially_covered: at(FRINGE_COLUMN),
        fully_covered: at(COVERED_COLUMN),
    }
}

/// One `wgpu::BlendFactor`, evaluated for a single channel.
///
/// `Src`/`Dst` take the channel itself, which is what the hardware does in
/// both the colour and the alpha component — in the alpha component the
/// channel IS the alpha.
fn factor(
    blend_factor: wgpu::BlendFactor,
    source: f32,
    source_alpha: f32,
    destination: f32,
    destination_alpha: f32,
) -> f32 {
    match blend_factor {
        wgpu::BlendFactor::Zero => 0.0,
        wgpu::BlendFactor::One => 1.0,
        wgpu::BlendFactor::Src => source,
        wgpu::BlendFactor::OneMinusSrc => 1.0 - source,
        wgpu::BlendFactor::SrcAlpha => source_alpha,
        wgpu::BlendFactor::OneMinusSrcAlpha => 1.0 - source_alpha,
        wgpu::BlendFactor::Dst => destination,
        wgpu::BlendFactor::OneMinusDst => 1.0 - destination,
        wgpu::BlendFactor::DstAlpha => destination_alpha,
        wgpu::BlendFactor::OneMinusDstAlpha => 1.0 - destination_alpha,
        unmodelled => panic!(
            "{unmodelled:?} reached this CPU blender, which models only the factors \
             `blend_state_for` emits; add it here rather than letting the oracle guess"
        ),
    }
}

/// `blend_state_for(mode)` applied on the CPU to a PREMULTIPLIED source and
/// destination.
pub(super) fn blend(mode: BlendMode, source: [f32; 4], destination: [f32; 4]) -> [f32; 4] {
    let state = blend_state_for(mode);
    let mut out = [0.0; 4];
    for channel in 0..4 {
        let component = if channel == 3 {
            state.alpha
        } else {
            state.color
        };
        assert_eq!(
            component.operation,
            wgpu::BlendOperation::Add,
            "{mode:?} uses a blend operation this CPU model does not implement"
        );
        out[channel] = factor(
            component.src_factor,
            source[channel],
            source[3],
            destination[channel],
            destination[3],
        ) * source[channel]
            + factor(
                component.dst_factor,
                source[channel],
                source[3],
                destination[channel],
                destination[3],
            ) * destination[channel];
    }
    out
}

/// Premultiplied `color`, scaled by `alpha_scale`.
pub(super) fn premultiplied(color: Color, alpha_scale: f32) -> [f32; 4] {
    let [r, g, b, a] = color.to_rgba_f32_array();
    let alpha = a * alpha_scale;
    [r * alpha, g * alpha, b * alpha, alpha]
}

/// The coverage-correct result: the mode applied at full strength, mixed with
/// the untouched destination by `coverage`.
///
/// This is the DEFINITION the correction must reproduce, not a restatement of
/// it — nothing here knows about a second blend source.
pub(super) fn coverage_correct(
    mode: BlendMode,
    source: Color,
    destination: Color,
    coverage: f32,
) -> [f32; 4] {
    let destination = premultiplied(destination, 1.0);
    let blended = blend(mode, premultiplied(source, 1.0), destination);
    std::array::from_fn(|channel| {
        destination[channel] + coverage * (blended[channel] - destination[channel])
    })
}

/// The result when coverage rides in the source alpha, which is what a device
/// without a second blend source can do and no more.
pub(super) fn coverage_folded(
    mode: BlendMode,
    source: Color,
    destination: Color,
    coverage: f32,
) -> [f32; 4] {
    blend(
        mode,
        premultiplied(source, coverage),
        premultiplied(destination, 1.0),
    )
}

/// Quantise a linear-space RGBA quadruple the way an `Rgba8Unorm` target does.
pub(super) fn as_bytes(channels: [f32; 4]) -> [u8; 4] {
    channels.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Assert one readback pixel against a predicted value, within [`TOLERANCE`].
pub(super) fn assert_pixel(actual: [u8; 4], expected: [f32; 4], what: &str) {
    let expected = as_bytes(expected);
    let within_tolerance = actual
        .iter()
        .zip(expected)
        .all(|(&got, want)| (i32::from(got) - i32::from(want)).abs() <= TOLERANCE);
    assert!(
        within_tolerance,
        "{what}: expected {expected:?} (±{TOLERANCE}), got {actual:?}"
    );
}
