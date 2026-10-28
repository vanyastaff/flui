//! What the window surface carries: the colour request, the pair the adapter
//! granted, and the transform from the renderer's working space to it.
//!
//! The renderer paints in one working space, extended-range sRGB-encoded
//! (ADR-0256). A surface that is not plain 8-bit sRGB is reached through the
//! presentation blit, which converts working pixels to the surface's gamut and
//! transfer function; [`ResolvedSurfaceColor`] is what that conversion needs
//! and what the application can read back.

use bytemuck::{Pod, Zeroable};
use flui_platform::{DisplayColor, DynamicRange, Headroom};

use crate::error::{EngineError, EngineResult};

/// The headroom assumed for an HDR display whose peak the platform does not
/// report, as a multiple of SDR white.
const DEFAULT_HDR_HEADROOM: f32 = 4.0;

/// SDR white in nits when it is mapped to an absolute-luminance signal (PQ),
/// the reference white of ITU-R BT.2408.
const PQ_REFERENCE_WHITE_NITS: f32 = 203.0;

/// The colour capability an application asks of its window surface.
///
/// A request, not a promise: the surface's capabilities decide, a request the
/// surface cannot meet falls back down the chain `Hdr` → `WideGamut` →
/// `Srgb8`, and [`Renderer::surface_color`](crate::Renderer::surface_color)
/// reports what was granted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SurfaceColor {
    /// 8 bits per channel in sRGB. Every surface offers it; the renderer
    /// paints straight into the swapchain.
    #[default]
    Srgb8,
    /// A standard-dynamic-range surface wider than sRGB (Display P3), where the
    /// platform offers one.
    WideGamut,
    /// An extended-range surface: values above SDR white drive brighter output,
    /// where the platform offers one and the display is in HDR mode.
    Hdr,
}

/// How the working-space pixels reach the surface.
///
/// The cases that are not [`Self::Identity`] go through the presentation blit
/// in an intermediate target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OutputEncoding {
    /// The surface holds the working pixels as they are.
    Identity,
    /// Convert in the blit.
    Transform(OutputTransform),
}

/// The surface colour space a blit converts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OutputTransform {
    pub(crate) gamut: Gamut,
    pub(crate) transfer: Transfer,
    /// The brightest value the display can show, as a multiple of SDR white;
    /// highlights above it are compressed toward it.
    pub(crate) headroom: f32,
}

/// Primaries of the output colour space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gamut {
    Srgb,
    DisplayP3,
    Rec2020,
}

/// Transfer function of the output colour space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transfer {
    /// sRGB-encoded, clamped to `0..=1` (an SDR surface).
    Srgb,
    /// sRGB-encoded, range kept.
    SrgbExtended,
    /// Linear, `1.0` = SDR white (scRGB).
    Linear,
    /// SMPTE ST 2084 over an absolute 10 000-nit range.
    Pq,
}

/// What a surface's colour is chosen from besides its capabilities: the
/// application's request and the colour the platform reports for the display
/// the window is on, which a recreated surface is resolved against again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColorRequest {
    pub(crate) requested: SurfaceColor,
    pub(crate) display: Option<DisplayColor>,
}

/// The colour configuration a surface was given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedSurfaceColor {
    requested: SurfaceColor,
    display: Option<DisplayColor>,
    granted: SurfaceColor,
    format: wgpu::TextureFormat,
    color_space: wgpu::SurfaceColorSpace,
    working_format: wgpu::TextureFormat,
    output: OutputEncoding,
}

impl ResolvedSurfaceColor {
    /// What the application asked for.
    #[must_use]
    pub const fn requested(&self) -> SurfaceColor {
        self.requested
    }

    /// What the surface delivers. Differs from [`requested`](Self::requested)
    /// when the surface could not meet it.
    #[must_use]
    pub const fn granted(&self) -> SurfaceColor {
        self.granted
    }

    /// Whether the request was met only in part.
    #[must_use]
    pub fn is_downgraded(&self) -> bool {
        self.requested != self.granted
    }

    /// The swapchain texture format.
    #[must_use]
    pub const fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The colour space the swapchain was configured with.
    #[must_use]
    pub const fn color_space(&self) -> wgpu::SurfaceColorSpace {
        self.color_space
    }

    /// The format the renderer paints in, which the swapchain format is
    /// converted from when they differ.
    #[must_use]
    pub const fn working_format(&self) -> wgpu::TextureFormat {
        self.working_format
    }

    /// How far above SDR white the display can show, as a multiple; `1.0` for
    /// an SDR surface.
    #[must_use]
    pub fn headroom(&self) -> f32 {
        match self.output {
            OutputEncoding::Identity => 1.0,
            OutputEncoding::Transform(transform) => transform.headroom,
        }
    }

    pub(crate) const fn output(&self) -> OutputEncoding {
        self.output
    }

    /// What this configuration was resolved from, for the surface that
    /// replaces this one.
    pub(crate) const fn request(&self) -> ColorRequest {
        ColorRequest {
            requested: self.requested,
            display: self.display,
        }
    }

    /// Choose the surface configuration for `requested` from what the surface
    /// offers.
    ///
    /// The display's state comes from `display`, the colour the platform
    /// reports for the display the window is on, where it reports one, and
    /// otherwise from `hdr`, what the surface reports. A display that is not
    /// in a high-range mode, or reports no headroom over SDR white, is treated
    /// as SDR even where the surface lists extended colour spaces. The
    /// headroom the tone mapping is bounded by is the platform's, then the
    /// surface's, then a default for an HDR display that reports no peak.
    /// `working_renderable` says whether the adapter can render and blend
    /// [`wgpu::TextureFormat::Rgba16Float`], the working format of every
    /// non-`Srgb8` configuration.
    ///
    /// # Errors
    /// [`EngineError::UnsupportedSurfaceColorConfiguration`] when the surface
    /// offers no 8-bit sRGB pair, the one every request can fall back to.
    pub fn resolve(
        requested: SurfaceColor,
        caps: &wgpu::SurfaceCapabilities,
        display: Option<DisplayColor>,
        hdr: &wgpu::DisplayHdrInfo,
        working_renderable: bool,
    ) -> EngineResult<Self> {
        let sdr = select_sdr(caps)?;
        let (display_is_hdr, headroom) = display_range(display, hdr);
        let offers =
            |format, space: wgpu::SurfaceColorSpaces| caps.color_spaces(format).contains(space);
        let working = wgpu::TextureFormat::Rgba16Float;
        let converted = |granted, format, color_space, gamut, transfer, headroom| Self {
            requested,
            display,
            granted,
            format,
            color_space,
            working_format: working,
            output: OutputEncoding::Transform(OutputTransform {
                gamut,
                transfer,
                headroom,
            }),
        };

        if requested == SurfaceColor::Hdr && working_renderable && display_is_hdr {
            use wgpu::{SurfaceColorSpace as Space, SurfaceColorSpaces as Spaces};
            let head = headroom.unwrap_or(DEFAULT_HDR_HEADROOM);
            let f16 = wgpu::TextureFormat::Rgba16Float;
            let candidates = [
                (
                    f16,
                    Space::ExtendedDisplayP3,
                    Spaces::EXTENDED_DISPLAY_P3,
                    Gamut::DisplayP3,
                    Transfer::SrgbExtended,
                ),
                (
                    f16,
                    Space::ExtendedSrgbLinear,
                    Spaces::EXTENDED_SRGB_LINEAR,
                    Gamut::Srgb,
                    Transfer::Linear,
                ),
                (
                    f16,
                    Space::ExtendedSrgb,
                    Spaces::EXTENDED_SRGB,
                    Gamut::Srgb,
                    Transfer::SrgbExtended,
                ),
                (
                    wgpu::TextureFormat::Rgb10a2Unorm,
                    Space::Bt2100Pq,
                    Spaces::BT2100_PQ,
                    Gamut::Rec2020,
                    Transfer::Pq,
                ),
            ];
            for (format, space, flag, gamut, transfer) in candidates {
                if offers(format, flag) {
                    return Ok(converted(
                        SurfaceColor::Hdr,
                        format,
                        space,
                        gamut,
                        transfer,
                        head,
                    ));
                }
            }
        }
        if requested != SurfaceColor::Srgb8 && working_renderable {
            for format in [
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
            ] {
                if offers(format, wgpu::SurfaceColorSpaces::DISPLAY_P3) {
                    return Ok(converted(
                        SurfaceColor::WideGamut,
                        format,
                        wgpu::SurfaceColorSpace::DisplayP3,
                        Gamut::DisplayP3,
                        Transfer::Srgb,
                        1.0,
                    ));
                }
            }
        }
        let SdrChoice {
            format,
            working_format,
            output,
        } = sdr;
        Ok(Self {
            requested,
            display,
            granted: SurfaceColor::Srgb8,
            format,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            working_format,
            output,
        })
    }
}

/// Whether the display shows extended range, and the headroom it reports:
/// the platform's answer where it gives a colour for the display, the
/// surface's otherwise.
///
/// A platform colour in standard range is SDR whatever the surface says. One
/// in high range takes the platform's headroom, or the surface's when the
/// platform reports no peak and the surface reports one over white. Without a
/// platform colour, a surface that reports no headroom is taken as HDR-capable
/// (wgpu reports `Some(1.0)` for a definitively-SDR display).
fn display_range(display: Option<DisplayColor>, hdr: &wgpu::DisplayHdrInfo) -> (bool, Option<f32>) {
    let surface = hdr.tone_map_headroom().filter(|h| h.is_finite());
    match display {
        Some(color) => (
            color.color_space.dynamic_range == DynamicRange::High,
            color
                .headroom
                .ok()
                .map(Headroom::get)
                .or_else(|| surface.filter(|&h| h > 1.0)),
        ),
        None => (surface.is_none_or(|h| h > 1.0), surface),
    }
}

/// The standard-dynamic-range sRGB configuration every request falls back to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SdrChoice {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) working_format: wgpu::TextureFormat,
    pub(crate) output: OutputEncoding,
}

/// Choose the sRGB configuration, in order of preference:
///
/// 1. `Bgra8Unorm` / `Rgba8Unorm`: `Color::to_rgba_f32_array` and the shaders
///    preserve encoded values, and a plain UNorm format stores those bytes
///    without another transfer function. The renderer paints into it directly.
/// 2. `Rgb10a2Unorm`, the same way.
/// 3. `Bgra8UnormSrgb` / `Rgba8UnormSrgb`: the hardware encodes on store, so
///    the renderer paints into an `Rgba8Unorm` working target and the
///    presentation blit writes the decoded (linear) values for the format to
///    encode again. The round trip is exact for 8-bit values.
///
/// An FP16/Auto surface can instead select `ExtendedSrgbLinear` and brighten
/// the same values, which is why an HDR surface is only ever chosen on request.
pub(crate) fn select_sdr(caps: &wgpu::SurfaceCapabilities) -> EngineResult<SdrChoice> {
    use wgpu::TextureFormat as Format;
    let srgb = |format| {
        caps.color_spaces(format)
            .contains(wgpu::SurfaceColorSpaces::SRGB)
    };
    for format in [Format::Bgra8Unorm, Format::Rgba8Unorm, Format::Rgb10a2Unorm] {
        if srgb(format) {
            return Ok(SdrChoice {
                format,
                working_format: format,
                output: OutputEncoding::Identity,
            });
        }
    }
    for format in [Format::Bgra8UnormSrgb, Format::Rgba8UnormSrgb] {
        if srgb(format) {
            return Ok(SdrChoice {
                format,
                working_format: Format::Rgba8Unorm,
                output: OutputEncoding::Transform(OutputTransform {
                    gamut: Gamut::Srgb,
                    transfer: Transfer::Linear,
                    headroom: 1.0,
                }),
            });
        }
    }
    Err(EngineError::UnsupportedSurfaceColorConfiguration {
        supported: caps.format_capabilities.clone(),
    })
}

/// Whether `adapter` can render to and blend the working format, which the
/// retained target also copies from and to.
pub(crate) fn working_format_renderable(adapter: &wgpu::Adapter) -> bool {
    let features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
    features.allowed_usages.contains(
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    ) && features
        .flags
        .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
}

/// The blit's uniform block; the layout `present_transform.wgsl` declares.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct OutputParams {
    /// Rows of the linear sRGB → output primaries matrix, each padded to a vec4.
    matrix: [[f32; 4]; 3],
    transfer: u32,
    headroom: f32,
    reference_white_nits: f32,
    _pad: f32,
}

impl OutputParams {
    pub(crate) fn new(transform: OutputTransform) -> Self {
        let rows = gamut_matrix(transform.gamut);
        Self {
            matrix: rows.map(|[r, g, b]| [r, g, b, 0.0]),
            transfer: match transform.transfer {
                Transfer::Srgb => 0,
                Transfer::SrgbExtended => 1,
                Transfer::Linear => 2,
                Transfer::Pq => 3,
            },
            headroom: transform.headroom,
            reference_white_nits: PQ_REFERENCE_WHITE_NITS,
            _pad: 0.0,
        }
    }
}

/// Linear sRGB → linear output primaries (D65 throughout).
const fn gamut_matrix(gamut: Gamut) -> [[f32; 3]; 3] {
    match gamut {
        Gamut::Srgb => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Gamut::DisplayP3 => [
            [0.822_462_1, 0.177_538, 0.0],
            [0.033_194_1, 0.966_805_8, 0.0],
            [0.017_082_7, 0.072_397_4, 0.910_519_9],
        ],
        Gamut::Rec2020 => [
            [0.627_404, 0.329_282, 0.043_313_6],
            [0.069_097_0, 0.919_54, 0.011_361_2],
            [0.016_391_6, 0.088_013_2, 0.895_595],
        ],
    }
}

/// Reference implementation of the blit's colour conversion, one premultiplied
/// pixel. The shader mirrors it; the readback test holds the two together.
#[cfg(test)]
pub(crate) fn convert_pixel(transform: OutputTransform, pixel: [f32; 4]) -> [f32; 4] {
    let alpha = pixel[3];
    let straight = if alpha > 0.0 {
        [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha]
    } else {
        [0.0; 3]
    };
    let linear = straight.map(srgb_decode);
    let linear = tone_map(linear, transform.headroom);
    let m = gamut_matrix(transform.gamut);
    let mapped = m.map(|row| row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2]);
    let encoded = mapped.map(|v| match transform.transfer {
        Transfer::Srgb => srgb_encode(v).clamp(0.0, 1.0),
        Transfer::SrgbExtended => srgb_encode(v),
        Transfer::Linear => v,
        Transfer::Pq => pq_encode(v.max(0.0) * PQ_REFERENCE_WHITE_NITS / 10_000.0),
    });
    [
        encoded[0] * alpha,
        encoded[1] * alpha,
        encoded[2] * alpha,
        alpha,
    ]
}

#[cfg(test)]
pub(crate) fn srgb_decode(v: f32) -> f32 {
    let a = v.abs();
    let l = if a <= 0.040_45 {
        a / 12.92
    } else {
        ((a + 0.055) / 1.055).powf(2.4)
    };
    l.copysign(v)
}

#[cfg(test)]
pub(crate) fn srgb_encode(v: f32) -> f32 {
    let a = v.abs();
    let e = if a <= 0.003_130_8 {
        a * 12.92
    } else {
        1.055 * a.powf(1.0 / 2.4) - 0.055
    };
    e.copysign(v)
}

#[cfg(test)]
fn pq_encode(y: f32) -> f32 {
    const M1: f32 = 0.159_301_76;
    const M2: f32 = 78.843_75;
    const C1: f32 = 0.835_937_5;
    const C2: f32 = 18.851_563;
    const C3: f32 = 18.687_5;
    let p = y.powf(M1);
    ((C1 + C2 * p) / (1.0 + C3 * p)).powf(M2)
}

/// Compress the brightest channel above SDR white toward `headroom`, scaling
/// all three so hue holds. Identity at or below `1.0`; a `headroom` of `1.0`
/// clips the peak to white.
#[cfg(test)]
pub(crate) fn tone_map(linear: [f32; 3], headroom: f32) -> [f32; 3] {
    let peak = linear[0].max(linear[1]).max(linear[2]);
    if peak <= 1.0 {
        return linear;
    }
    let room = headroom - 1.0;
    let mapped = if room <= 0.0 {
        1.0
    } else {
        let x = (peak - 1.0) / room;
        1.0 + room * x / (1.0 + x)
    };
    let scale = mapped / peak;
    linear.map(|v| v * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};

    fn caps(pairs: &[(Format, Spaces)]) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: pairs.iter().map(|&(format, _)| format).collect(),
            format_capabilities: pairs
                .iter()
                .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities {
                    format,
                    color_spaces,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn hdr_display(headroom: f32) -> wgpu::DisplayHdrInfo {
        wgpu::DisplayHdrInfo {
            headroom: Some(wgpu::DisplayHeadroom {
                current: Some(headroom),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn resolve(
        requested: SurfaceColor,
        pairs: &[(Format, Spaces)],
        hdr: &wgpu::DisplayHdrInfo,
    ) -> ResolvedSurfaceColor {
        ResolvedSurfaceColor::resolve(requested, &caps(pairs), None, hdr, true).expect("SDR pair")
    }

    /// [`resolve`] on a display the platform reports `display` for.
    fn resolve_on(
        display: DisplayColor,
        pairs: &[(Format, Spaces)],
        hdr: &wgpu::DisplayHdrInfo,
    ) -> ResolvedSurfaceColor {
        ResolvedSurfaceColor::resolve(SurfaceColor::Hdr, &caps(pairs), Some(display), hdr, true)
            .expect("SDR pair")
    }

    #[test]
    fn the_platform_colour_decides_the_display_state_over_the_surface_report() {
        use flui_platform::{CapabilityAbsence, ColorGamut};

        let offered = [
            SDR,
            (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
            (Format::Bgra8Unorm, Spaces::SRGB.union(Spaces::DISPLAY_P3)),
        ];
        // The platform says HDR is off; the surface's report of headroom does
        // not turn it on.
        let sdr = resolve_on(
            DisplayColor::standard(ColorGamut::Srgb),
            &offered,
            &hdr_display(4.0),
        );
        assert_eq!(sdr.granted(), SurfaceColor::WideGamut);

        // The platform reports an extended-range display with 2.5 times white
        // while the surface reports no headroom yet (Apple before a layer
        // asks for extended range): the platform decides, with its headroom.
        let platform_headroom = Headroom::new(2.5).expect("headroom");
        let hdr = resolve_on(
            DisplayColor::high(ColorGamut::DisplayP3, Ok(platform_headroom)),
            &offered,
            &hdr_display(1.0),
        );
        assert_eq!(hdr.granted(), SurfaceColor::Hdr);
        assert_eq!(hdr.headroom(), 2.5);
        assert_eq!(
            hdr.request().display,
            Some(DisplayColor::high(
                ColorGamut::DisplayP3,
                Ok(platform_headroom)
            )),
            "a recreated surface resolves against the same display report"
        );

        // High range without a reported peak: the surface's headroom, then
        // the default.
        let unreported =
            DisplayColor::high(ColorGamut::Rec2020, Err(CapabilityAbsence::NotOnThisDevice));
        assert_eq!(
            resolve_on(unreported, &offered, &hdr_display(3.0)).headroom(),
            3.0
        );
        assert_eq!(
            resolve_on(unreported, &offered, &wgpu::DisplayHdrInfo::default()).headroom(),
            DEFAULT_HDR_HEADROOM
        );
    }

    const SDR: (Format, Spaces) = (Format::Bgra8Unorm, Spaces::SRGB);

    #[test]
    fn srgb8_ignores_what_wider_surfaces_offer() {
        let offered = [
            SDR,
            (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
            (Format::Bgra8Unorm, Spaces::SRGB.union(Spaces::DISPLAY_P3)),
        ];
        let resolved = resolve(SurfaceColor::Srgb8, &offered, &hdr_display(4.0));
        assert_eq!(resolved.granted(), SurfaceColor::Srgb8);
        assert!(!resolved.is_downgraded());
        assert_eq!(resolved.output(), OutputEncoding::Identity);
        assert_eq!(resolved.working_format(), resolved.format());
    }

    #[test]
    fn hdr_takes_the_first_extended_pair_in_preference_order() {
        let display = hdr_display(3.0);
        let linear = [SDR, (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)];
        let resolved = resolve(SurfaceColor::Hdr, &linear, &display);
        assert_eq!(resolved.granted(), SurfaceColor::Hdr);
        assert_eq!(
            resolved.color_space(),
            wgpu::SurfaceColorSpace::ExtendedSrgbLinear
        );
        assert_eq!(resolved.working_format(), Format::Rgba16Float);
        assert_eq!(resolved.headroom(), 3.0);

        let apple = [
            SDR,
            (
                Format::Rgba16Float,
                Spaces::EXTENDED_SRGB_LINEAR.union(Spaces::EXTENDED_DISPLAY_P3),
            ),
        ];
        assert_eq!(
            resolve(SurfaceColor::Hdr, &apple, &display).color_space(),
            wgpu::SurfaceColorSpace::ExtendedDisplayP3
        );

        let web = [SDR, (Format::Rgba16Float, Spaces::EXTENDED_SRGB)];
        assert_eq!(
            resolve(SurfaceColor::Hdr, &web, &display).color_space(),
            wgpu::SurfaceColorSpace::ExtendedSrgb
        );

        let hdr10 = [SDR, (Format::Rgb10a2Unorm, Spaces::BT2100_PQ)];
        let resolved = resolve(SurfaceColor::Hdr, &hdr10, &display);
        assert_eq!(resolved.format(), Format::Rgb10a2Unorm);
        assert_eq!(resolved.color_space(), wgpu::SurfaceColorSpace::Bt2100Pq);
    }

    #[test]
    fn hdr_on_a_display_without_headroom_falls_to_wide_gamut_then_srgb() {
        let offered = [
            (Format::Bgra8Unorm, Spaces::SRGB.union(Spaces::DISPLAY_P3)),
            (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
        ];
        let sdr_display = hdr_display(1.0);
        let resolved = resolve(SurfaceColor::Hdr, &offered, &sdr_display);
        assert_eq!(resolved.granted(), SurfaceColor::WideGamut);
        assert!(resolved.is_downgraded());
        assert_eq!(resolved.color_space(), wgpu::SurfaceColorSpace::DisplayP3);

        let resolved = resolve(SurfaceColor::Hdr, &[SDR], &sdr_display);
        assert_eq!(resolved.granted(), SurfaceColor::Srgb8);
        assert!(resolved.is_downgraded());
    }

    #[test]
    fn an_unreported_headroom_still_allows_hdr_with_the_default() {
        let offered = [SDR, (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)];
        let resolved = resolve(
            SurfaceColor::Hdr,
            &offered,
            &wgpu::DisplayHdrInfo::default(),
        );
        assert_eq!(resolved.granted(), SurfaceColor::Hdr);
        assert_eq!(resolved.headroom(), DEFAULT_HDR_HEADROOM);
    }

    #[test]
    fn wide_gamut_never_picks_an_hdr_space() {
        let offered = [SDR, (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)];
        let resolved = resolve(SurfaceColor::WideGamut, &offered, &hdr_display(4.0));
        assert_eq!(resolved.granted(), SurfaceColor::Srgb8);
        assert!(resolved.is_downgraded());
    }

    #[test]
    fn an_adapter_that_cannot_blend_the_working_format_gets_srgb8() {
        let offered = [
            (Format::Bgra8Unorm, Spaces::SRGB.union(Spaces::DISPLAY_P3)),
            (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
        ];
        let resolved = ResolvedSurfaceColor::resolve(
            SurfaceColor::Hdr,
            &caps(&offered),
            None,
            &hdr_display(4.0),
            false,
        )
        .expect("SDR pair");
        assert_eq!(resolved.granted(), SurfaceColor::Srgb8);
    }

    #[test]
    fn a_surface_with_only_srgb_formats_is_painted_through_an_unorm_working_target() {
        for srgb_format in [Format::Bgra8UnormSrgb, Format::Rgba8UnormSrgb] {
            let resolved = resolve(
                SurfaceColor::Srgb8,
                &[(srgb_format, Spaces::SRGB)],
                &wgpu::DisplayHdrInfo::default(),
            );
            assert_eq!(resolved.granted(), SurfaceColor::Srgb8);
            assert_eq!(resolved.format(), srgb_format);
            assert_eq!(resolved.color_space(), wgpu::SurfaceColorSpace::Srgb);
            assert_eq!(resolved.working_format(), Format::Rgba8Unorm);
            assert!(matches!(resolved.output(), OutputEncoding::Transform(_)));
        }
    }

    #[test]
    fn a_plain_unorm_pair_beats_an_srgb_format_and_ten_bit_beats_neither() {
        let both = [
            (Format::Bgra8UnormSrgb, Spaces::SRGB),
            (Format::Rgb10a2Unorm, Spaces::SRGB),
            (Format::Rgba8Unorm, Spaces::SRGB),
        ];
        let resolved = resolve(SurfaceColor::Srgb8, &both, &wgpu::DisplayHdrInfo::default());
        assert_eq!(resolved.format(), Format::Rgba8Unorm);
        let without = &both[..2];
        let resolved = resolve(
            SurfaceColor::Srgb8,
            without,
            &wgpu::DisplayHdrInfo::default(),
        );
        assert_eq!(resolved.format(), Format::Rgb10a2Unorm);
        assert_eq!(resolved.output(), OutputEncoding::Identity);
    }

    /// What the painter wrote in sRGB-encoded bytes is what the `*Srgb`
    /// swapchain stores: the blit writes linear values and the hardware
    /// encodes them again.
    #[cfg(feature = "testing")]
    #[test]
    fn an_srgb_format_surface_stores_the_bytes_the_painter_wrote() {
        let Some((device, queue)) =
            crate::test_support::try_test_device_and_queue("srgb format surface")
        else {
            return;
        };
        let resolved = resolve(
            SurfaceColor::Srgb8,
            &[(Format::Rgba8UnormSrgb, Spaces::SRGB)],
            &wgpu::DisplayHdrInfo::default(),
        );
        let OutputEncoding::Transform(transform) = resolved.output() else {
            panic!("an sRGB-format surface is converted");
        };
        let width = 256u32;
        let (source, _) = crate::test_support::create_sampleable_target(
            &device,
            "working",
            width,
            1,
            resolved.working_format(),
        );
        let texels: Vec<u8> = (0..width)
            .flat_map(|v| [v as u8, 255 - v as u8, (v as u8) / 2, 255])
            .collect();
        queue.write_texture(
            source.as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(1),
            },
            source.size(),
        );
        let (target, view) = crate::test_support::create_sampleable_target(
            &device,
            "surface",
            width,
            1,
            resolved.format(),
        );
        crate::offscreen::OffscreenRenderer::new(
            std::sync::Arc::clone(&device),
            std::sync::Arc::clone(&queue),
        )
        .present_to_surface(
            &source,
            &target,
            &view,
            OutputEncoding::Transform(transform),
        )
        .expect("presentation submits");
        let stored = crate::test_support::readback_bytes(&device, &queue, &target, width, 1);
        for (index, (stored, wrote)) in stored.iter().zip(&texels).enumerate() {
            assert!(
                stored.abs_diff(*wrote) <= 1,
                "byte {index}: stored {stored}, painter wrote {wrote}"
            );
        }
    }
    #[test]
    fn a_surface_without_the_sdr_pair_is_refused_whatever_was_asked() {
        let only_hdr = caps(&[(Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)]);
        for requested in [
            SurfaceColor::Srgb8,
            SurfaceColor::WideGamut,
            SurfaceColor::Hdr,
        ] {
            let error =
                ResolvedSurfaceColor::resolve(requested, &only_hdr, None, &hdr_display(4.0), true)
                    .expect_err("no SDR pair");
            assert!(matches!(
                error,
                EngineError::UnsupportedSurfaceColorConfiguration { .. }
            ));
        }
    }

    #[test]
    fn tone_mapping_is_the_identity_up_to_white_and_bounded_by_the_headroom() {
        assert_eq!(tone_map([0.2, 0.9, 1.0], 4.0), [0.2, 0.9, 1.0]);
        let mapped = tone_map([8.0, 4.0, 2.0], 4.0);
        assert!(mapped[0] < 4.0 && mapped[0] > 1.0, "{mapped:?}");
        // Hue holds: the ratios between channels are kept.
        assert!((mapped[0] / mapped[1] - 2.0).abs() < 1e-5);
        assert!((mapped[1] / mapped[2] - 2.0).abs() < 1e-5);
        // Monotonic, and continuous in slope at white.
        let a = tone_map([1.5, 0.0, 0.0], 4.0)[0];
        let b = tone_map([3.0, 0.0, 0.0], 4.0)[0];
        assert!(1.0 < a && a < b && b < 4.0);
        let near = tone_map([1.001, 0.0, 0.0], 4.0)[0];
        assert!((near - 1.001).abs() < 1e-4);
        // No headroom: the peak clips to white.
        assert_eq!(tone_map([3.0, 1.5, 0.0], 1.0), [1.0, 0.5, 0.0]);
    }

    #[test]
    fn the_reference_conversion_matches_known_values() {
        let p3 = OutputTransform {
            gamut: Gamut::DisplayP3,
            transfer: Transfer::Srgb,
            headroom: 1.0,
        };
        // White and black are fixed points of every gamut conversion.
        let white = convert_pixel(p3, [1.0; 4]);
        assert!(white.iter().all(|c| (c - 1.0).abs() < 1e-4), "{white:?}");
        // sRGB red is inside P3: it lands at a lower red and a little green.
        let red = convert_pixel(p3, [1.0, 0.0, 0.0, 1.0]);
        assert!(
            (red[0] - 0.917_5).abs() < 2e-3 && (red[1] - 0.200_3).abs() < 2e-3,
            "{red:?}"
        );

        let scrgb = OutputTransform {
            gamut: Gamut::Srgb,
            transfer: Transfer::Linear,
            headroom: 4.0,
        };
        // Mid grey 188/255 encodes ~0.5 linear; alpha stays premultiplied.
        let grey = convert_pixel(scrgb, [0.5 * 0.735_4, 0.5 * 0.735_4, 0.5 * 0.735_4, 0.5]);
        assert!((grey[0] - 0.5 * 0.5).abs() < 2e-3, "{grey:?}");

        let pq = OutputTransform {
            gamut: Gamut::Rec2020,
            transfer: Transfer::Pq,
            headroom: 4.0,
        };
        // SDR white at 203 nits is PQ 0.5807.
        let white = convert_pixel(pq, [1.0; 4]);
        assert!((white[0] - 0.580_7).abs() < 2e-3, "{white:?}");
    }

    #[cfg(feature = "testing")]
    fn f16_bits(value: f32) -> u16 {
        // Round to nearest; the samples are normal, in range and few.
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let mantissa = bits & 0x007f_ffff;
        if value == 0.0 {
            return sign;
        }
        assert!((1..31).contains(&exponent), "{value} is not a normal f16");
        let rounded = (mantissa + 0x1000) >> 13;
        sign | (((exponent as u32) << 10) + rounded) as u16
    }

    #[cfg(feature = "testing")]
    fn f16_value(bits: u16) -> f32 {
        let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
        let exponent = i32::from((bits >> 10) & 0x1f);
        let mantissa = f32::from(bits & 0x3ff);
        match exponent {
            0 => sign * mantissa * 2f32.powi(-24),
            31 => sign * f32::INFINITY,
            _ => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
        }
    }

    /// The presentation blit and `convert_pixel` agree on every transfer,
    /// gamut, an over-white highlight and a translucent pixel.
    #[cfg(feature = "testing")]
    #[test]
    fn the_presentation_blit_matches_the_reference_conversion() {
        let Some((device, queue)) =
            crate::test_support::try_test_device_and_queue("present transform")
        else {
            return;
        };
        let samples: [[f32; 4]; 7] = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.2, 0.5, 0.8, 1.0],
            [0.5, 0.25, 0.125, 0.5],
            // A highlight above SDR white, in the working space's extended range.
            [2.5, 1.5, 1.0, 1.0],
            [-0.1, 0.4, 0.4, 1.0],
        ];
        let width = samples.len() as u32;
        let format = wgpu::TextureFormat::Rgba16Float;
        let (source, _) = crate::test_support::create_sampleable_target(
            &device,
            "present source",
            width,
            1,
            format,
        );
        let texels: Vec<u16> = samples.iter().flatten().map(|&v| f16_bits(v)).collect();
        queue.write_texture(
            source.as_image_copy(),
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(1),
            },
            source.size(),
        );
        let mut presenter = crate::offscreen::OffscreenRenderer::new(
            std::sync::Arc::clone(&device),
            std::sync::Arc::clone(&queue),
        );
        let transforms = [
            OutputTransform {
                gamut: Gamut::DisplayP3,
                transfer: Transfer::Srgb,
                headroom: 1.0,
            },
            OutputTransform {
                gamut: Gamut::Srgb,
                transfer: Transfer::Linear,
                headroom: 4.0,
            },
            OutputTransform {
                gamut: Gamut::Srgb,
                transfer: Transfer::SrgbExtended,
                headroom: 2.0,
            },
            OutputTransform {
                gamut: Gamut::DisplayP3,
                transfer: Transfer::SrgbExtended,
                headroom: 4.0,
            },
            OutputTransform {
                gamut: Gamut::Rec2020,
                transfer: Transfer::Pq,
                headroom: 4.0,
            },
        ];
        for transform in transforms {
            let (target, view) = crate::test_support::create_sampleable_target(
                &device,
                "present target",
                width,
                1,
                format,
            );
            presenter
                .present_to_surface(
                    &source,
                    &target,
                    &view,
                    OutputEncoding::Transform(transform),
                )
                .expect("presentation submits");
            let bytes = crate::test_support::readback_texels(&device, &queue, &target, width, 1, 8);
            let (halves, _) = bytes.as_chunks::<2>();
            for (index, sample) in samples.iter().enumerate() {
                let expected = convert_pixel(transform, *sample);
                for channel in 0..4 {
                    let actual = f16_value(u16::from_le_bytes(halves[index * 4 + channel]));
                    let tolerance = 3e-3 + 2e-3 * expected[channel].abs();
                    assert!(
                        (actual - expected[channel]).abs() <= tolerance,
                        "{transform:?} sample {index} channel {channel}: {actual} vs {}",
                        expected[channel]
                    );
                }
            }
        }
    }

    /// Painting in the `Rgba16Float` working target and presenting to an 8-bit
    /// Display P3 surface lands each swatch where the reference conversion
    /// puts the sRGB value the painter was given, translucent blends included.
    #[cfg(feature = "testing")]
    #[test]
    fn a_wide_gamut_frame_reaches_the_surface_through_the_working_target() {
        use flui_foundation::geometry::Rect;
        use flui_painting::Paint;
        use flui_painting::styling::Color;

        let Some((device, queue)) =
            crate::test_support::try_test_device_and_queue("wide gamut frame")
        else {
            return;
        };
        let size = 64;
        let working = wgpu::TextureFormat::Rgba16Float;
        let surface_format = wgpu::TextureFormat::Rgba8Unorm;
        let (source, view) =
            crate::test_support::create_sampleable_target(&device, "working", size, size, working);
        crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
        let mut painter = crate::painter::WgpuPainter::with_shared_device(
            std::sync::Arc::clone(&device),
            std::sync::Arc::clone(&queue),
            working,
            (size, size),
        );
        let swatches = [
            (
                Color::rgb(229, 57, 53),
                [229.0 / 255.0, 57.0 / 255.0, 53.0 / 255.0, 1.0],
            ),
            (Color::rgb(255, 0, 0), [1.0, 0.0, 0.0, 1.0]),
            (
                Color::rgb(30, 144, 255),
                [30.0 / 255.0, 144.0 / 255.0, 1.0, 1.0],
            ),
            // Over the black underlay: half-covered grey, premultiplied.
            (Color::rgba(128, 128, 128, 128), [0.25, 0.25, 0.25, 1.0]),
        ];
        for (index, (color, _)) in swatches.iter().enumerate() {
            painter.draw_rect(
                Rect::from_xywh(f64::from(index as f32 * 10.0), 0.0, 10.0, 64.0),
                &Paint::fill(*color),
            );
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .render_to_view(&view, &mut encoder)
            .expect("paint swatches");
        queue.submit([encoder.finish()]);

        let (target, target_view) = crate::test_support::create_sampleable_target(
            &device,
            "surface",
            size,
            size,
            surface_format,
        );
        let transform = OutputTransform {
            gamut: Gamut::DisplayP3,
            transfer: Transfer::Srgb,
            headroom: 1.0,
        };
        crate::offscreen::OffscreenRenderer::new(
            std::sync::Arc::clone(&device),
            std::sync::Arc::clone(&queue),
        )
        .present_to_surface(
            &source,
            &target,
            &target_view,
            OutputEncoding::Transform(transform),
        )
        .expect("presentation submits");
        let bytes = crate::test_support::readback_bytes(&device, &queue, &target, size, size);
        for (index, (_, painted)) in swatches.iter().enumerate() {
            let expected = convert_pixel(transform, *painted);
            let offset = (32 * size as usize + index * 10 + 5) * 4;
            for channel in 0..3 {
                let actual = f32::from(bytes[offset + channel]) / 255.0;
                assert!(
                    (actual - expected[channel]).abs() <= 2.5 / 255.0,
                    "swatch {index} channel {channel}: {actual} vs {}",
                    expected[channel]
                );
            }
        }
        // The wide-gamut conversion is not a copy: P3 red is not 255 in red alone.
        let red = 32 * size as usize * 4 + 15 * 4;
        assert!(
            bytes[red] < 250 && bytes[red + 1] > 30,
            "{:?}",
            &bytes[red..red + 4]
        );
    }
}
