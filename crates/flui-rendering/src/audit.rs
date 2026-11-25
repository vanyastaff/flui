//! Rules over a finished frame (ADR-0400 §5): what an accessibility audit reads from the
//! frame's draw commands, never from its pixels.
//!
//! [`PipelineOwner::audit_text_contrast`] judges each text the frame paints by WCAG 2.2's
//! contrast minimum (success criterion 1.4.3): the contrast ratio of the colour its glyphs
//! paint in against the colour painted behind them is at least 4.5:1, or 3:1 for large text
//! (at least 24 logical px, or at least 18.66 px bold, CSS's 18 pt and 14 pt). The ratio is
//! compared as computed: WCAG does not round it, and the colours come exact from the scene, so
//! there is no sampling error for a tolerance to absorb (Flutter's guideline allows 0.01 because
//! it samples pixels). The development agent's `audit.a11y` is its caller.
//!
//! Both colours come from the frame's scene, read in paint order, in the logical pixels the
//! frame was laid out in:
//!
//! - the text's colour is its run's span colour, or the paragraph command's, composited over
//!   what is behind it when it is translucent;
//! - what is behind it is the last solid fill painted before it that covers the whole visible
//!   box of the text (the line boxes its glyphs are set in, inside every clip and save layer's
//!   bounds), with each translucent solid fill painted over that one, covering the box too,
//!   composited on top;
//! - each translucent solid fill painted after the text that covers its whole visible box (a
//!   scrim) is composited over both colours.
//!
//! A text whose colours the commands do not determine is not passed; it is reported
//! [`ContrastOutcome::NotChecked`] with one of the reasons [`ContrastUnknown`] names:
//!
//! - anything else painted over its box before it and after the last covering opaque fill (an
//!   image, a gradient, a stroke, another text, a fill that covers only part of it, a blend
//!   mode other than source-over): [`ContrastUnknown::UnresolvedBackdrop`];
//! - no opaque fill behind it at all (the surface's clear colour is the host's, not the
//!   frame's): [`ContrastUnknown::NothingOpaqueBehind`];
//! - the text under a layer or transform the walk does not resolve, or a fill behind it that
//!   only such a layer, a clip of a shape the walk does not resolve or a paint that changes what
//!   is already painted keeps from being one known colour: [`ContrastUnknown::UnresolvedEffect`];
//! - anything but a covering translucent fill painted over its box after it:
//!   [`ContrastUnknown::PaintedOver`];
//! - bold and regular spans mixed at a size where only bold text is large (from 18.66 px to
//!   under 24 px), when its runs do not say which is which: [`ContrastUnknown::MixedWeight`];
//! - a run whose font size, as painted, is not a finite positive number:
//!   [`ContrastUnknown::NonFinite`].
//!
//! A text under a clip of any shape is judged against what is behind its box: the clip hides
//! part of the text, not the fill behind what shows. A glyph run whose colour is fully
//! transparent paints nothing to read and is not judged, and a text with no other run is left
//! out of the answer: it is neither a finding nor unchecked.
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
use backdrop::{Backdrop, Boldness, PaintedText};

/// The ratio normal text needs (WCAG 2.2, 1.4.3).
const NORMAL_TEXT_CONTRAST: f64 = 4.5;

/// The ratio large text needs (WCAG 2.2, 1.4.3).
const LARGE_TEXT_CONTRAST: f64 = 3.0;

/// The smallest large text, in logical px: 18 pt.
const LARGE_TEXT_PX: f64 = 24.0;

/// The smallest large bold text, in logical px: 14 pt.
const LARGE_BOLD_TEXT_PX: f64 = 18.66;

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
    /// The colour its glyphs show in: composited over the colour painted behind them, with the
    /// translucent fills painted over the text composited on top.
    pub foreground: Color,
    /// The opaque colour behind it, with the translucent fills painted over the text composited
    /// on top.
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
    /// The text sits under a layer or transform the rule does not resolve (opacity below 1 or
    /// a blend mode other than source-over, a colour or image filter, a shader mask, a save
    /// layer whose paint is not one opaque colour, a transform other than a scale, a flip or a
    /// quarter turn), or a fill painted behind it is kept from being one known colour over its
    /// box by such a layer or transform, by a clip of a path, a superellipse or a difference,
    /// by a backdrop filter, or by a save layer that composites with another blend mode.
    UnresolvedEffect,
    /// Something painted over the text's box after it, other than a translucent solid fill
    /// covering the whole box, changes what shows: an opaque fill hides the text, and an image,
    /// a gradient, a stroke, another text or a fill over part of it changes part of it.
    PaintedOver,
    /// The text mixes bold and regular spans at a size where only bold text is large, and its
    /// runs do not say which is which.
    MixedWeight,
    /// A run's font size, as painted, is not a finite positive number.
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
                "the text, or the fill behind it, is under an effect the rule does not resolve \
                 (opacity, a filter, a mask, a save layer, a skewing or rotating transform, a \
                 path or superellipse clip, a backdrop filter)"
            }
            Self::PaintedOver => {
                "something painted over the text after it hides or changes part of what shows"
            }
            Self::MixedWeight => {
                "the text mixes bold and regular spans at a size where only bold counts as large"
            }
            Self::NonFinite => "the text's font size is not a finite positive number",
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
        let painted = backdrop::painted_texts(version, frame)?;
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
                .collect::<Vec<TextContrast>>(),
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
    let backdrop = match &text.backdrop {
        Ok(backdrop) => backdrop,
        Err(unknown) => return ContrastOutcome::NotChecked(*unknown),
    };
    let mut worst_meeting: Option<ContrastReading> = None;
    let mut worst_below: Option<ContrastReading> = None;
    let mut unknown = None;
    for run in &text.runs {
        match reading(run.color, run.font_size, text.bold, backdrop) {
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

/// One run's reading: `color` at `font_size` against `backdrop`.
fn reading(
    color: Color,
    font_size: f64,
    bold: Boldness,
    backdrop: &Backdrop,
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
    // Colours are normalized to 0..=1 (`Color`), so the ratio is finite, from 1 to 21.
    let foreground = backdrop.shown(color.blend_over(backdrop.behind));
    let background = backdrop.shown(backdrop.behind);
    let ratio = contrast_ratio(foreground, background);
    Ok(ContrastReading {
        ratio,
        required,
        foreground,
        background,
        font_size,
        bold,
    })
}

/// Whether `reading` meets its requirement (module docs: no tolerance).
fn meets(reading: ContrastReading) -> bool {
    reading.ratio >= reading.required
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

/// WCAG's relative luminance of an opaque sRGB colour, computed in `f64` from the colour's `f32`
/// channels.
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

#[cfg(test)]
mod tests {
    use flui_painting::styling::Color;

    use super::backdrop::{Backdrop, Boldness};
    use super::{ContrastUnknown, reading};

    /// A run whose size is not a finite positive number is not judged. The walk cannot hand one
    /// over through a frame (it drops a text whose box is not finite before it reads a size), so
    /// the answer is pinned on the reading itself.
    #[test]
    fn a_run_whose_size_is_not_finite_and_positive_is_not_judged() {
        let white = Backdrop {
            behind: Color::WHITE,
            over: Vec::new(),
        };
        for size in [f64::NAN, f64::INFINITY, 0.0, -16.0] {
            assert_eq!(
                reading(Color::BLACK, size, Boldness::Regular, &white),
                Err(ContrastUnknown::NonFinite),
                "{size}"
            );
        }
        assert!(reading(Color::BLACK, 16.0, Boldness::Regular, &white).is_ok());
    }
}
