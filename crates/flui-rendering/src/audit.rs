//! Rules over a finished frame (ADR-0400 §5): what an accessibility audit reads from the
//! frame's draw commands, never from its pixels.
//!
//! [`PipelineOwner::audit_text_contrast`] judges each text the frame paints by WCAG 2.2's
//! contrast minimum (success criterion 1.4.3): the contrast ratio of the colour its glyphs
//! paint in against the colour painted behind them is at least 4.5:1, or 3:1 for large text
//! (at least 24 logical px, or at least 18.66 px bold, CSS's 18 pt and 14 pt), with a 0.01
//! tolerance for rounding. The development agent's `audit.a11y` and the testing kit call it, so
//! the rule is written once.
//!
//! Both colours come from the frame's scene, read in paint order:
//!
//! - the text's colour is its run's span colour, or the paragraph command's, composited over
//!   what is behind it when it is translucent;
//! - what is behind it is the last solid fill painted before it that covers the whole visible
//!   box of the text (the line boxes its glyphs are set in, inside every clip), with each
//!   translucent solid fill painted over that one, covering the box too, composited on top.
//!
//! A text whose background the commands do not determine is not passed: anything else painted
//! over its box before it and after the last covering opaque fill (an image, a gradient, a
//! stroke, another text, a fill that covers only part of it, a blend mode other than
//! source-over), no opaque fill behind it at all (the surface's clear colour is the host's, not
//! the frame's), or the text or its backdrop under a layer the walk does not resolve (opacity, a
//! colour or image filter, a shader mask, a backdrop filter, a path clip, a transform that turns
//! rectangles into other shapes) is reported [`ContrastOutcome::NotChecked`] with why.
//!
//! The module only reads a frame the pipeline already produced: it changes no layout, paint,
//! layer or phase.

mod backdrop;

use std::fmt;

use flui_foundation::RenderId;
use flui_layer::SceneVersion;
use flui_painting::styling::Color;
use rustc_hash::FxHashSet;

use crate::pipeline::PipelineOwner;
use backdrop::{Boldness, PaintedText};

/// The ratio normal text needs (WCAG 2.2, 1.4.3).
const NORMAL_TEXT_CONTRAST: f64 = 4.5;

/// The ratio large text needs (WCAG 2.2, 1.4.3).
const LARGE_TEXT_CONTRAST: f64 = 3.0;

/// The smallest large text, in logical px: 18 pt.
const LARGE_TEXT_PX: f64 = 24.0;

/// The smallest large bold text, in logical px: 14 pt.
const LARGE_BOLD_TEXT_PX: f64 = 18.66;

/// How far below its requirement a ratio may fall and still meet it: the rounding a ratio
/// printed to two decimals hides.
const CONTRAST_TOLERANCE: f64 = 0.01;

/// A font size this close to a threshold, in logical px, is at it: run sizes are `f32`, and
/// 18.66 has no exact `f32`.
const SIZE_EPSILON: f64 = 1e-3;

/// One text a frame paints, judged by the contrast rule (module docs).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextContrast {
    /// The render object whose paint drew the text.
    pub painter: RenderId,
    /// The nearest render object, the painter or one above it, that contributes a node to the
    /// committed semantics tree: the element a finding is about. `None` while semantics is off
    /// or no render object above the painter has a node.
    pub element: Option<RenderId>,
    /// The text, as shaped (an ellipsis included).
    pub text: String,
    /// What the rule found.
    pub outcome: ContrastOutcome,
}

/// What the contrast rule found for one text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ContrastOutcome {
    /// Every run of the text meets its requirement; the reading is the run closest to failing.
    Meets(ContrastReading),
    /// A run falls below its requirement; the reading is the run furthest below.
    Below(ContrastReading),
    /// The rule could not judge the text, and says why.
    NotChecked(ContrastUnknown),
}

/// The contrast of one run of a text against what is behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ContrastReading {
    /// The WCAG contrast ratio, from 1 to 21.
    pub ratio: f64,
    /// The ratio its size needs: 4.5, or 3 for large text.
    pub required: f64,
    /// The colour its glyphs show in, composited over the background.
    pub foreground: Color,
    /// The opaque colour behind it.
    pub background: Color,
    /// Its font size, in logical px, as painted (a scale above it applied).
    pub font_size: f64,
    /// Whether it counts as bold for the large-text threshold.
    pub bold: bool,
}

/// Why the contrast rule could not judge a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContrastUnknown {
    /// No opaque solid fill painted before the text covers its box: what shows behind it is
    /// the surface's clear colour, which the frame does not hold.
    NothingOpaqueBehind,
    /// Something painted over the text's box before it does not resolve to one colour: an
    /// image, a gradient, a stroke, another text, a fill that covers only part of the box, or a
    /// blend mode other than source-over.
    UnresolvedBackdrop,
    /// The text, or what is painted behind it, sits under a layer the rule does not resolve:
    /// opacity, a colour or image filter, a shader mask, a backdrop filter, a path clip, or a
    /// transform that turns rectangles into other shapes.
    UnresolvedEffect,
    /// The text mixes bold and regular spans at a size where only bold text is large, and its
    /// runs do not say which is which.
    MixedWeight,
    /// A font size or a colour is not finite.
    NonFinite,
}

impl fmt::Display for ContrastUnknown {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::NothingOpaqueBehind => {
                "no opaque fill is painted behind the text, so its background is the surface's"
            }
            Self::UnresolvedBackdrop => {
                "what is painted behind the text is not one solid colour (an image, a gradient, \
                 another paint over part of it)"
            }
            Self::UnresolvedEffect => {
                "the text or its background is under an opacity, filter, mask, path clip or \
                 rotation the rule does not resolve"
            }
            Self::MixedWeight => {
                "the text mixes bold and regular spans at a size where only bold counts as large"
            }
            Self::NonFinite => "the text's size or colour is not finite",
        })
    }
}

impl PipelineOwner {
    /// Every text this owner's frame in `version` paints, judged by the contrast rule, in paint
    /// order (module docs). `None` when `version` holds no frame of this owner: before its first
    /// paint was applied, or a version of another lineage.
    ///
    /// Reads `version` and the committed render and semantics trees; changes nothing.
    #[must_use]
    pub fn audit_text_contrast(&self, version: &SceneVersion) -> Option<Vec<TextContrast>> {
        let frame = self.scene_frame_node()?;
        let painted = backdrop::painted_texts(version, frame, self.device_pixel_ratio().get())?;
        let with_nodes: FxHashSet<RenderId> = self.semantics_owner().map_or_default(|owner| {
            owner
                .tree()
                .iter()
                .filter_map(|(_, node)| node.source_render_id())
                .collect::<FxHashSet<RenderId>>()
        });
        Some(
            painted
                .into_iter()
                .map(|text| {
                    let element = self.element_of(text.painter, &with_nodes);
                    TextContrast {
                        painter: text.painter,
                        element,
                        outcome: judge(&text),
                        text: text.text,
                    }
                })
                .collect(),
        )
    }

    /// The painter, or the nearest render object above it, that contributes a semantics node.
    fn element_of(&self, painter: RenderId, with_nodes: &FxHashSet<RenderId>) -> Option<RenderId> {
        let tree = self.render_tree();
        // The tree's parent links end at its root, and each step goes up one level.
        std::iter::successors(Some(painter), |&id| tree.parent(id))
            .find(|id| with_nodes.contains(id))
    }
}

/// The outcome for one painted text: its runs against its backdrop.
fn judge(text: &PaintedText) -> ContrastOutcome {
    let background = match text.backdrop {
        Ok(background) => background,
        Err(unknown) => return ContrastOutcome::NotChecked(unknown),
    };
    let mut worst_meeting: Option<ContrastReading> = None;
    let mut worst_below: Option<ContrastReading> = None;
    let mut unknown = None;
    for run in &text.runs {
        match reading(run.color, run.font_size, text.bold, background) {
            Ok(reading) if meets(reading) => {
                if worst_meeting.is_none_or(|worst| margin(reading) < margin(worst)) {
                    worst_meeting = Some(reading);
                }
            }
            Ok(reading) => {
                if worst_below.is_none_or(|worst| margin(reading) < margin(worst)) {
                    worst_below = Some(reading);
                }
            }
            Err(why) => unknown = unknown.or(Some(why)),
        }
    }
    match (worst_below, unknown, worst_meeting) {
        (Some(below), ..) => ContrastOutcome::Below(below),
        (None, Some(why), _) => ContrastOutcome::NotChecked(why),
        (None, None, Some(meeting)) => ContrastOutcome::Meets(meeting),
        // A text with no glyph runs paints nothing to read.
        (None, None, None) => ContrastOutcome::NotChecked(ContrastUnknown::UnresolvedBackdrop),
    }
}

/// One run's reading: `color` at `font_size` over the opaque `background`.
fn reading(
    color: Color,
    font_size: f64,
    bold: Boldness,
    background: Color,
) -> Result<ContrastReading, ContrastUnknown> {
    if !font_size.is_finite() || font_size <= 0.0 {
        return Err(ContrastUnknown::NonFinite);
    }
    let (required, bold) = if font_size + SIZE_EPSILON >= LARGE_TEXT_PX {
        (LARGE_TEXT_CONTRAST, bold == Boldness::Bold)
    } else if font_size + SIZE_EPSILON >= LARGE_BOLD_TEXT_PX {
        match bold {
            Boldness::Bold => (LARGE_TEXT_CONTRAST, true),
            Boldness::Regular => (NORMAL_TEXT_CONTRAST, false),
            Boldness::Mixed => return Err(ContrastUnknown::MixedWeight),
        }
    } else {
        (NORMAL_TEXT_CONTRAST, bold == Boldness::Bold)
    };
    let foreground = color.blend_over(background);
    let ratio = contrast_ratio(foreground, background);
    if !ratio.is_finite() {
        return Err(ContrastUnknown::NonFinite);
    }
    Ok(ContrastReading {
        ratio,
        required,
        foreground,
        background,
        font_size,
        bold,
    })
}

/// Whether `reading` meets its requirement, within the tolerance.
fn meets(reading: ContrastReading) -> bool {
    reading.ratio + CONTRAST_TOLERANCE >= reading.required
}

/// How far above its requirement a reading is; negative below it.
fn margin(reading: ContrastReading) -> f64 {
    reading.ratio - reading.required
}

/// WCAG's contrast ratio of two opaque colours: `(L1 + 0.05) / (L2 + 0.05)`, the lighter's
/// relative luminance over the darker's, from 1 to 21.
fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

/// WCAG's relative luminance of an opaque sRGB colour, computed in `f64` so the 0.01
/// tolerance is not spent on rounding.
fn relative_luminance(color: Color) -> f64 {
    let channel = |value: f32| {
        let value = f64::from(value);
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red_f32())
        + 0.7152 * channel(color.green_f32())
        + 0.0722 * channel(color.blue_f32())
}
