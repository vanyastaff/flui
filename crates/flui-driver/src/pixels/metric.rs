//! The per-pixel metric: pixelmatch's YIQ distance, as pixelmatch 5 computes it (the version
//! Playwright vendors for `toHaveScreenshot`, blending a translucent pixel over white; egui's
//! kittest uses the same metric), so a threshold means here what it means there.
//!
//! Two pixels are blended over white by their alpha, converted to YIQ, and their distance is the
//! weighted sum `0.5053 y² + 0.299 i² + 0.1957 q²` (Kotsarenko and Ramos, "Measuring perceived
//! color difference using YIQ NTSC transmission color space in mobile applications", 2010).
//! [`MAX_YIQ_DELTA`] is the largest distance any two pixels have, and a threshold `t` lets pixels
//! within `MAX_YIQ_DELTA * t²` pass as equal. An anti-aliased pixel (one whose neighbours change
//! brightness on both sides, while a neighbour of the extremes is flat in both images) counts as
//! a difference unless the tolerance excuses it.

use std::fmt;

use super::Image;
use crate::error::{Error, Result};

/// The largest YIQ distance two pixels can have: pixelmatch's normalizing constant.
pub const MAX_YIQ_DELTA: f64 = 35215.0;

/// The colour a differing pixel takes in the diff image.
pub const DIFF_COLOR: [u8; 4] = [255, 0, 0, 255];

/// The colour an excused anti-aliased pixel takes in the diff image.
pub const ANTIALIASED_COLOR: [u8; 4] = [255, 255, 0, 255];

/// How much a pixel of the diff image keeps of the expected image's brightness: pixelmatch's
/// default, so the diff shows faintly what it is of.
const DIFF_FADE: f64 = 0.1;

/// What a comparison lets pass.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Tolerance {
    /// Per pixel: two pixels within `MAX_YIQ_DELTA * threshold²` are the same. From 0 (exact) to
    /// 1; 0.1 by default.
    pub threshold: f64,
    /// How many differing pixels pass; 0 by default.
    pub max_diff_pixels: u64,
    /// Which share of the compared pixels may differ, from 0 to 1. When set, the larger of the
    /// two budgets is the one that holds, as Playwright takes them.
    pub max_diff_ratio: Option<f64>,
    /// Whether an anti-aliased pixel is excused: off by default, since FLUI's own rasterizer
    /// is what a baseline pins.
    pub ignore_antialiasing: bool,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            max_diff_pixels: 0,
            max_diff_ratio: None,
            ignore_antialiasing: false,
        }
    }
}

impl Tolerance {
    /// No differing pixel at all: threshold 0.
    #[must_use]
    pub fn exact() -> Self {
        Self::default().threshold(0.0)
    }

    /// With the per-pixel `threshold`.
    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    /// Letting `pixels` differing pixels pass.
    #[must_use]
    pub fn max_diff_pixels(mut self, pixels: u64) -> Self {
        self.max_diff_pixels = pixels;
        self
    }

    /// Letting `ratio` of the compared pixels differ.
    #[must_use]
    pub fn max_diff_ratio(mut self, ratio: f64) -> Self {
        self.max_diff_ratio = Some(ratio);
        self
    }

    /// Excusing anti-aliased pixels when `ignore`.
    #[must_use]
    pub fn ignore_antialiasing(mut self, ignore: bool) -> Self {
        self.ignore_antialiasing = ignore;
        self
    }

    fn check(&self) -> Result<()> {
        let ratio_ok = self
            .max_diff_ratio
            .is_none_or(|ratio| (0.0..=1.0).contains(&ratio));
        if (0.0..=1.0).contains(&self.threshold) && ratio_ok {
            return Ok(());
        }
        Err(Error::invalid_argument(format!(
            "a tolerance's threshold and ratio lie between 0 and 1, not {} and {:?}",
            self.threshold, self.max_diff_ratio
        )))
    }
}

/// How two images of one size differ.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub struct Difference {
    /// Pixels past the threshold, anti-aliased ones included unless excused.
    pub differing: u64,
    /// Anti-aliased pixels past the threshold that the tolerance excused.
    pub antialiased: u64,
    /// The pixels compared.
    pub compared: u64,
    /// The expected image faded, with each differing pixel [`DIFF_COLOR`] and each excused one
    /// [`ANTIALIASED_COLOR`].
    pub diff: Image,
}

impl Difference {
    /// Whether `tolerance` lets the difference pass.
    #[must_use]
    pub fn within(&self, tolerance: &Tolerance) -> bool {
        if self.differing <= tolerance.max_diff_pixels {
            return true;
        }
        tolerance.max_diff_ratio.is_some_and(|ratio| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "pixel counts of a surface are far below 2^53, where f64 stops being exact"
            )]
            let share = self.differing as f64 / self.compared.max(1) as f64;
            share <= ratio
        })
    }
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} of {} pixels differ", self.differing, self.compared)?;
        if self.antialiased > 0 {
            write!(f, " ({} anti-aliased ones excused)", self.antialiased)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Difference")
            .field("differing", &self.differing)
            .field("antialiased", &self.antialiased)
            .field("compared", &self.compared)
            .finish_non_exhaustive()
    }
}

/// How `actual` differs from `expected`, pixel by pixel.
///
/// # Errors
///
/// [`Error::InvalidArgument`] when the images differ in size, or the tolerance is out of range.
pub fn compare(expected: &Image, actual: &Image, tolerance: &Tolerance) -> Result<Difference> {
    tolerance.check()?;
    if (expected.width(), expected.height()) != (actual.width(), actual.height()) {
        return Err(Error::invalid_argument(format!(
            "a {} x {} image compared with a {} x {} one",
            expected.width(),
            expected.height(),
            actual.width(),
            actual.height()
        )));
    }
    let max_delta = MAX_YIQ_DELTA * tolerance.threshold * tolerance.threshold;
    let mut diff = expected.clone();
    let mut differing = 0_u64;
    let mut antialiased = 0_u64;
    for y in 0..expected.height() {
        for x in 0..expected.width() {
            let (Some(a), Some(b)) = (expected.pixel(x, y), actual.pixel(x, y)) else {
                continue;
            };
            let delta = color_delta(a, b);
            if delta.abs() > max_delta {
                let excused = tolerance.ignore_antialiasing
                    && (antialiased_at(expected, x, y, actual)
                        || antialiased_at(actual, x, y, expected));
                if excused {
                    antialiased += 1;
                    diff.set_pixel(x, y, ANTIALIASED_COLOR);
                } else {
                    differing += 1;
                    diff.set_pixel(x, y, DIFF_COLOR);
                }
            } else {
                diff.set_pixel(x, y, faded(a));
            }
        }
    }
    Ok(Difference {
        differing,
        antialiased,
        compared: u64::from(expected.width()) * u64::from(expected.height()),
        diff,
    })
}

/// The YIQ distance between two pixels, each blended over white by its alpha: negative when the
/// first is the brighter, as pixelmatch signs it.
#[must_use]
pub fn color_delta(first: [u8; 4], second: [u8; 4]) -> f64 {
    if first == second {
        return 0.0;
    }
    let [r1, g1, b1] = blended(first);
    let [r2, g2, b2] = blended(second);
    let y1 = brightness(r1, g1, b1);
    let y2 = brightness(r2, g2, b2);
    let luma = y1 - y2;
    let hue = in_phase(r1, g1, b1) - in_phase(r2, g2, b2);
    let saturation = quadrature(r1, g1, b1) - quadrature(r2, g2, b2);
    let delta = 0.5053 * luma * luma + 0.299 * hue * hue + 0.1957 * saturation * saturation;
    if y1 > y2 { -delta } else { delta }
}

/// The brightness difference alone, the first pixel's minus the second's.
fn brightness_delta(a: [u8; 4], b: [u8; 4]) -> f64 {
    if a == b {
        return 0.0;
    }
    let [r1, g1, b1] = blended(a);
    let [r2, g2, b2] = blended(b);
    brightness(r1, g1, b1) - brightness(r2, g2, b2)
}

/// The pixel's colour over white, by its alpha.
fn blended(pixel: [u8; 4]) -> [f64; 3] {
    let alpha = f64::from(pixel[3]) / 255.0;
    let blend = |channel: u8| 255.0 + (f64::from(channel) - 255.0) * alpha;
    [blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]
}

fn brightness(r: f64, g: f64, b: f64) -> f64 {
    r * 0.298_895_31 + g * 0.586_622_47 + b * 0.114_482_23
}

fn in_phase(r: f64, g: f64, b: f64) -> f64 {
    r * 0.595_977_99 - g * 0.274_176_10 - b * 0.321_801_89
}

fn quadrature(r: f64, g: f64, b: f64) -> f64 {
    r * 0.211_470_17 - g * 0.522_617_11 + b * 0.311_146_94
}

/// A pixel the diff image keeps of the expected image: its brightness, faded towards white.
fn faded(pixel: [u8; 4]) -> [u8; 4] {
    let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(f64::from);
    let alpha = DIFF_FADE * f64::from(pixel[3]) / 255.0;
    let value = 255.0 + (brightness(r, g, b) - 255.0) * alpha;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 and rounded first"
    )]
    let level = value.round().clamp(0.0, 255.0) as u8;
    [level, level, level, 255]
}

/// The pixels around (`x`, `y`) that are in `image`: up to eight, fewer at an edge.
fn neighbours(image: &Image, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
    let (width, height) = (image.width(), image.height());
    let xs = x.saturating_sub(1)..=x.saturating_add(1).min(width.saturating_sub(1));
    let ys = y.saturating_sub(1)..=y.saturating_add(1).min(height.saturating_sub(1));
    xs.flat_map(move |nx| ys.clone().map(move |ny| (nx, ny)))
        .filter(move |&(nx, ny)| (nx, ny) != (x, y))
}

/// Whether (`x`, `y`) sits on the image's edge, which pixelmatch counts as one flat neighbour.
fn on_edge(image: &Image, x: u32, y: u32) -> bool {
    x == 0 || y == 0 || x + 1 >= image.width() || y + 1 >= image.height()
}

/// Whether the pixel at (`x`, `y`) of `image` is anti-aliasing, judged against `other`
/// (pixelmatch's `antialiased`): at most two of its neighbours are as bright as it, some are
/// brighter and some darker, and the darkest or the brightest of them sits in a flat area in both
/// images.
fn antialiased_at(image: &Image, x: u32, y: u32, other: &Image) -> bool {
    let Some(center) = image.pixel(x, y) else {
        return false;
    };
    let mut flat = usize::from(on_edge(image, x, y));
    let mut darkest: Option<(f64, (u32, u32))> = None;
    let mut brightest: Option<(f64, (u32, u32))> = None;
    for (nx, ny) in neighbours(image, x, y) {
        let Some(pixel) = image.pixel(nx, ny) else {
            continue;
        };
        let delta = brightness_delta(center, pixel);
        if delta == 0.0 {
            flat += 1;
            if flat > 2 {
                return false;
            }
        } else if delta < darkest.map_or(0.0, |(min, _)| min) {
            darkest = Some((delta, (nx, ny)));
        } else if delta > brightest.map_or(0.0, |(max, _)| max) {
            brightest = Some((delta, (nx, ny)));
        }
    }
    let (Some((_, low)), Some((_, high))) = (darkest, brightest) else {
        return false;
    };
    let flat_in_both =
        |(px, py): (u32, u32)| has_many_siblings(image, px, py) && has_many_siblings(other, px, py);
    flat_in_both(low) || flat_in_both(high)
}

/// Whether the pixel at (`x`, `y`) has more than two neighbours of exactly its colour, the
/// image's edge counting as one.
fn has_many_siblings(image: &Image, x: u32, y: u32) -> bool {
    let Some(center) = image.pixel(x, y) else {
        return false;
    };
    let mut same = usize::from(on_edge(image, x, y));
    for (nx, ny) in neighbours(image, x, y) {
        if image.pixel(nx, ny) == Some(center) {
            same += 1;
            if same > 2 {
                return true;
            }
        }
    }
    false
}
