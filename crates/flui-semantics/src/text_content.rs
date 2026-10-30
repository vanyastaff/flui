//! A text field's text as assistive technology reads it: runs of characters
//! with their positions, word starts and lines, and the selection between
//! them (ADR-0204, the semantics-model text model).
//!
//! AccessKit reads text through `TextRun` children of the field. A run is a
//! stretch of one line in one direction, of at most
//! [`MAX_RUN_CHARACTERS`] characters, each character a `u8` byte length of
//! UTF-8. A position in that model, `(run, character)`, is a
//! [`TextRunPosition`]; the field's editor works in byte offsets of its value
//! ([`TextSelection`](crate::TextSelection)). The two are different types, and
//! [`TextContent`] converts between them.
//!
//! Runs are published under synthetic identities the pipeline assigns
//! ([`TextContent::with_run_identities`]); content without them is withheld
//! as a whole, runs and selection together, and reported.

use std::ops::Range;

use flui_foundation::geometry::{Matrix4, Rect};
use smol_str::SmolStr;

use crate::identity::AccessibilityNodeId;
use crate::properties::TextDirection;
use crate::regions::SemanticsRegionId;

/// The most characters one run holds: AccessKit's word starts are `u8`
/// indices into the run's characters.
pub const MAX_RUN_CHARACTERS: usize = 256;

/// A place in a field's runs: before character `character` of run `run`, or
/// at the run's end when `character` is its character count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRunPosition {
    /// The run, in the field's order.
    pub run: usize,
    /// The character within the run.
    pub character: usize,
}

impl TextRunPosition {
    /// Before character `character` of run `run`.
    #[must_use]
    pub const fn new(run: usize, character: usize) -> Self {
        Self { run, character }
    }
}

/// A selection in a field's runs: `anchor` stays put while the selection
/// grows, `focus` is the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRunSelection {
    /// Where the selection starts.
    pub anchor: TextRunPosition,
    /// Where it ends: the caret.
    pub focus: TextRunPosition,
}

impl TextRunSelection {
    /// From `anchor` to `focus`.
    #[must_use]
    pub const fn new(anchor: TextRunPosition, focus: TextRunPosition) -> Self {
        Self { anchor, focus }
    }

    /// A caret at `at`.
    #[must_use]
    pub const fn collapsed(at: TextRunPosition) -> Self {
        Self {
            anchor: at,
            focus: at,
        }
    }
}

/// What a run is built from: the layout's account of one stretch of a line.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextRunParts {
    /// The run's text: the mask, for a field that hides its input.
    pub text: SmolStr,
    /// Where the run starts in the field's value, in bytes.
    pub start: usize,
    /// The visual line the run lies on, from 0.
    pub line: usize,
    /// The run's direction.
    pub direction: TextDirection,
    /// The run's bounds, in the field's coordinate space.
    pub rect: Rect<f64>,
    /// Each character's length in UTF-8 bytes, in logical order.
    pub character_lengths: Vec<u8>,
    /// Each character's offset from the run's start edge, in the run's
    /// direction.
    pub character_positions: Vec<f32>,
    /// Each character's advance.
    pub character_widths: Vec<f32>,
    /// The characters that start a word, ascending.
    pub word_starts: Vec<u8>,
}

impl TextRunParts {
    /// A run of `text` starting at byte `start` of the field's value, on line
    /// `line`, with no characters described yet.
    #[must_use]
    pub fn new(
        text: impl Into<SmolStr>,
        start: usize,
        line: usize,
        direction: TextDirection,
        rect: Rect<f64>,
    ) -> Self {
        Self {
            text: text.into(),
            start,
            line,
            direction,
            rect,
            character_lengths: Vec::new(),
            character_positions: Vec::new(),
            character_widths: Vec::new(),
            word_starts: Vec::new(),
        }
    }
}

/// Why a run, a field's content or a position in it was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextRunError {
    /// The character lengths do not add up to the run's text.
    #[error("the character lengths add up to {lengths} bytes, the run's text has {text}")]
    LengthsDoNotCoverText {
        /// The sum of the lengths.
        lengths: usize,
        /// The text's length in bytes.
        text: usize,
    },
    /// A character is empty or ends inside a `char`.
    #[error("character {character} is empty or ends inside a UTF-8 sequence")]
    SplitsCharacter {
        /// The character.
        character: usize,
    },
    /// The run holds more than [`MAX_RUN_CHARACTERS`] characters.
    #[error("a run holds {count} characters, more than {MAX_RUN_CHARACTERS}")]
    TooManyCharacters {
        /// How many it holds.
        count: usize,
    },
    /// Positions or widths are not one per character.
    #[error("{characters} characters, {positions} positions and {widths} widths")]
    MetricsMismatch {
        /// The characters.
        characters: usize,
        /// The positions.
        positions: usize,
        /// The widths.
        widths: usize,
    },
    /// A position, width or bound is not finite.
    #[error("a run's geometry is not finite")]
    NonFiniteGeometry,
    /// Word starts are not ascending, or name a character the run lacks.
    #[error("word start {index} is out of order or past the run's characters")]
    InvalidWordStart {
        /// Its index in the word starts.
        index: usize,
    },
    /// A field has no run: an empty field has one empty run.
    #[error("a field's text has no run")]
    NoRuns,
    /// A run does not start where the one before it ends, or the first run
    /// does not start at the value's start.
    #[error("run {run} does not start where the previous run ends")]
    RunsNotContiguous {
        /// The run.
        run: usize,
    },
    /// A run lies on an earlier line than the one before it.
    #[error("run {run} lies on an earlier line than the run before it")]
    LinesDecrease {
        /// The run.
        run: usize,
    },
    /// A position names a run or a character the content lacks.
    #[error("position ({run}, {character}) is outside the text")]
    PositionOutOfRange {
        /// The run.
        run: usize,
        /// The character.
        character: usize,
    },
    /// The identities are not one per run.
    #[error("{identities} identities for {runs} runs")]
    IdentityCountMismatch {
        /// The runs.
        runs: usize,
        /// The identities.
        identities: usize,
    },
}

/// One run a field publishes: built with [`TextRun::try_new`], which checks
/// it against AccessKit's model.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    parts: TextRunParts,
}

impl TextRun {
    /// Checks `parts`: the lengths cover the text on `char` boundaries, at
    /// most [`MAX_RUN_CHARACTERS`] characters, one finite position and width
    /// each, and ascending word starts within the characters.
    ///
    /// # Errors
    ///
    /// The [`TextRunError`] naming the first violation.
    pub fn try_new(parts: TextRunParts) -> Result<Self, TextRunError> {
        let count = parts.character_lengths.len();
        if count > MAX_RUN_CHARACTERS {
            return Err(TextRunError::TooManyCharacters { count });
        }
        let mut end = 0usize;
        for (character, &length) in parts.character_lengths.iter().enumerate() {
            end += usize::from(length);
            if length == 0 || !parts.text.is_char_boundary(end.min(parts.text.len())) {
                return Err(TextRunError::SplitsCharacter { character });
            }
        }
        if end != parts.text.len() {
            return Err(TextRunError::LengthsDoNotCoverText {
                lengths: end,
                text: parts.text.len(),
            });
        }
        if parts.character_positions.len() != count || parts.character_widths.len() != count {
            return Err(TextRunError::MetricsMismatch {
                characters: count,
                positions: parts.character_positions.len(),
                widths: parts.character_widths.len(),
            });
        }
        let finite_rect = [
            parts.rect.left(),
            parts.rect.top(),
            parts.rect.right(),
            parts.rect.bottom(),
        ]
        .iter()
        .all(|edge| edge.is_finite());
        if !finite_rect
            || !parts
                .character_positions
                .iter()
                .chain(&parts.character_widths)
                .all(|metric| metric.is_finite())
        {
            return Err(TextRunError::NonFiniteGeometry);
        }
        let mut previous: Option<u8> = None;
        for (index, &start) in parts.word_starts.iter().enumerate() {
            if usize::from(start) >= count || previous.is_some_and(|previous| start <= previous) {
                return Err(TextRunError::InvalidWordStart { index });
            }
            previous = Some(start);
        }
        Ok(Self { parts })
    }

    /// The run's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.parts.text
    }

    /// Its bytes in the field's value.
    #[must_use]
    pub fn byte_range(&self) -> Range<usize> {
        self.parts.start..self.parts.start + self.parts.text.len()
    }

    /// The visual line it lies on.
    #[must_use]
    pub fn line(&self) -> usize {
        self.parts.line
    }

    /// Its direction.
    #[must_use]
    pub fn direction(&self) -> TextDirection {
        self.parts.direction
    }

    /// Its bounds in the field's coordinate space.
    #[must_use]
    pub fn rect(&self) -> Rect<f64> {
        self.parts.rect
    }

    /// Each character's length in UTF-8 bytes.
    #[must_use]
    pub fn character_lengths(&self) -> &[u8] {
        &self.parts.character_lengths
    }

    /// Each character's offset from the run's start edge.
    #[must_use]
    pub fn character_positions(&self) -> &[f32] {
        &self.parts.character_positions
    }

    /// Each character's advance.
    #[must_use]
    pub fn character_widths(&self) -> &[f32] {
        &self.parts.character_widths
    }

    /// The characters that start a word.
    #[must_use]
    pub fn word_starts(&self) -> &[u8] {
        &self.parts.word_starts
    }

    /// How many characters it holds.
    #[must_use]
    pub fn character_count(&self) -> usize {
        self.parts.character_lengths.len()
    }

    /// The byte of the field's value before `character`, or after the last
    /// character when `character` is the count.
    fn byte_offset(&self, character: usize) -> Option<usize> {
        let lengths = self.parts.character_lengths.get(..character)?;
        Some(
            self.parts.start
                + lengths
                    .iter()
                    .map(|&length| usize::from(length))
                    .sum::<usize>(),
        )
    }

    /// The character that starts at byte `byte` of the field's value, or the
    /// count when `byte` is the run's end. `None` inside a character or
    /// outside the run.
    fn character_at(&self, byte: usize) -> Option<usize> {
        let mut at = self.parts.start;
        for (character, &length) in self.parts.character_lengths.iter().enumerate() {
            if at == byte {
                return Some(character);
            }
            at += usize::from(length);
        }
        (at == byte).then_some(self.character_count())
    }
}

/// A field's text as runs, and its selection.
///
/// Runs cover the value from its first byte without gaps, on lines that
/// never go back. An empty field holds one empty run, so its caret has a
/// node to sit on.
#[derive(Debug, Clone, PartialEq)]
pub struct TextContent {
    runs: Box<[TextRun]>,
    selection: Option<TextRunSelection>,
    identities: Option<Box<[AccessibilityNodeId]>>,
}

impl TextContent {
    /// The content of `runs`, with no selection.
    ///
    /// # Errors
    ///
    /// [`TextRunError::NoRuns`], [`TextRunError::RunsNotContiguous`] or
    /// [`TextRunError::LinesDecrease`].
    pub fn try_new(runs: Vec<TextRun>) -> Result<Self, TextRunError> {
        if runs.is_empty() {
            return Err(TextRunError::NoRuns);
        }
        let mut end = 0;
        let mut line = 0;
        for (run, text) in runs.iter().enumerate() {
            if text.byte_range().start != end {
                return Err(TextRunError::RunsNotContiguous { run });
            }
            if text.line() < line {
                return Err(TextRunError::LinesDecrease { run });
            }
            end = text.byte_range().end;
            line = text.line();
        }
        Ok(Self {
            runs: runs.into_boxed_slice(),
            selection: None,
            identities: None,
        })
    }

    /// This content with `selection`.
    ///
    /// # Errors
    ///
    /// [`TextRunError::PositionOutOfRange`] when either end names a run or a
    /// character past the run's end.
    pub fn with_selection(mut self, selection: TextRunSelection) -> Result<Self, TextRunError> {
        self.byte_offset(selection.anchor)?;
        self.byte_offset(selection.focus)?;
        self.selection = Some(selection);
        Ok(self)
    }

    /// This content published under `identities`, one per run in order: the
    /// pipeline assigns them from the presentation's identity source.
    ///
    /// # Errors
    ///
    /// [`TextRunError::IdentityCountMismatch`].
    pub fn with_run_identities(
        mut self,
        identities: Vec<SemanticsRegionId>,
    ) -> Result<Self, TextRunError> {
        if identities.len() != self.runs.len() {
            return Err(TextRunError::IdentityCountMismatch {
                runs: self.runs.len(),
                identities: identities.len(),
            });
        }
        self.identities = Some(
            identities
                .into_iter()
                .map(SemanticsRegionId::accessibility_id)
                .collect(),
        );
        Ok(self)
    }

    /// The runs, in order.
    #[must_use]
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// The selection, if any.
    #[must_use]
    pub fn selection(&self) -> Option<TextRunSelection> {
        self.selection
    }

    /// The identities the runs publish under, once assigned.
    #[must_use]
    pub fn run_identities(&self) -> Option<&[AccessibilityNodeId]> {
        self.identities.as_deref()
    }

    /// The byte of the field's value at `position`.
    ///
    /// # Errors
    ///
    /// [`TextRunError::PositionOutOfRange`].
    pub fn byte_offset(&self, position: TextRunPosition) -> Result<usize, TextRunError> {
        self.runs
            .get(position.run)
            .and_then(|run| run.byte_offset(position.character))
            .ok_or(TextRunError::PositionOutOfRange {
                run: position.run,
                character: position.character,
            })
    }

    /// The position of byte `byte` of the field's value: the start of the run
    /// that begins there, or the end of the last run at the value's end.
    /// `None` inside a character or past the end.
    #[must_use]
    pub fn position_at(&self, byte: usize) -> Option<TextRunPosition> {
        self.runs.iter().enumerate().find_map(|(index, run)| {
            let range = run.byte_range();
            let last = index + 1 == self.runs.len();
            let reaches =
                range.contains(&byte) || (byte == range.end && (last || range.is_empty()));
            reaches
                .then(|| run.character_at(byte))
                .flatten()
                .map(|character| TextRunPosition::new(index, character))
        })
    }

    /// The position of the published character boundary at or before byte
    /// `byte`: a byte inside a character snaps to that character's start, and
    /// one past the text's end to the end. `None` only for content with no
    /// runs.
    #[must_use]
    pub fn position_at_or_before(&self, byte: usize) -> Option<TextRunPosition> {
        let (index, run) = self
            .runs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, run)| run.byte_range().start <= byte)
            .or_else(|| self.runs.first().map(|run| (0, run)))?;
        let mut at = run.parts.start;
        let mut character = 0;
        for &length in &run.parts.character_lengths {
            let next = at.saturating_add(usize::from(length));
            if next > byte {
                break;
            }
            at = next;
            character += 1;
        }
        Some(TextRunPosition::new(index, character))
    }

    /// `selection` in bytes of the field's value.
    ///
    /// # Errors
    ///
    /// [`TextRunError::PositionOutOfRange`].
    pub fn selection_bytes(
        &self,
        selection: TextRunSelection,
    ) -> Result<crate::TextSelection, TextRunError> {
        Ok(crate::TextSelection {
            anchor: self.byte_offset(selection.anchor)?,
            focus: self.byte_offset(selection.focus)?,
        })
    }
}

impl TextContent {
    /// This content with its geometry carried through `transform`, from the
    /// space of the render object that declared it into the space its node
    /// is published in: each run's bounds become the box around its moved
    /// corners, and positions and widths scale with the transform's
    /// horizontal scale. `None` when the moved geometry is not finite.
    #[must_use]
    pub fn transformed(&self, transform: &Matrix4) -> Option<Self> {
        let point = |x: f64, y: f64| {
            let (x, y) = transform.transform_point(x, y);
            (x.is_finite() && y.is_finite()).then_some((x, y))
        };
        let (origin_x, origin_y) = point(0.0, 0.0)?;
        let (unit_x, unit_y) = point(1.0, 0.0)?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "AccessKit's character metrics are f32; the scale is checked finite after"
        )]
        let scale = (unit_x - origin_x).hypot(unit_y - origin_y) as f32;
        if !scale.is_finite() {
            return None;
        }
        let mut runs = Vec::with_capacity(self.runs.len());
        for run in &self.runs {
            let rect = run.parts.rect;
            let corners = [
                point(rect.left(), rect.top())?,
                point(rect.right(), rect.top())?,
                point(rect.right(), rect.bottom())?,
                point(rect.left(), rect.bottom())?,
            ];
            let (mut left, mut top, mut right, mut bottom) = (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            );
            for (x, y) in corners {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
            let mut parts = run.parts.clone();
            parts.rect = Rect::from_ltrb(left, top, right, bottom);
            for metric in parts
                .character_positions
                .iter_mut()
                .chain(&mut parts.character_widths)
            {
                *metric *= scale;
                if !metric.is_finite() {
                    return None;
                }
            }
            runs.push(TextRun { parts });
        }
        Some(Self {
            runs: runs.into_boxed_slice(),
            selection: self.selection,
            identities: self.identities.clone(),
        })
    }
}
