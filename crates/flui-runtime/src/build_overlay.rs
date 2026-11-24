//! The build banner a presentation draws over its scene while a development tool reports a
//! build of the application (`build.report`, ADR-0395): a chip in the top-right corner while
//! a build runs, a panel along the bottom with the compiler's errors when it failed.
//!
//! The report reaches the presentation's [`BuildBanner`] through the agent's annotate path
//! (`ui_runtime::agent_serve`), which hands a handler this slot and nothing else of the
//! application. A frame lays the report out for the surface it draws ([`BuildBanner::banner`]),
//! and the presentation publishes the result as a node of its own after the performance
//! overlay's, recorded as that readout is: its labels shaped through the UI runtime's
//! [`TextContext`], a clip to its bounds, under the device-pixel-ratio scale
//! (`presentation_scene`). The application never sees it, and it never touches the
//! application's input: it is in no render, semantics or hit-test tree, so a press on it reaches
//! the widget under it as it would were the banner not drawn, and a driver finds that widget as
//! reachable as it is. The banner stays until the next report.
//!
//! Owner thread only (ADR-0323 §1): the slot is cells.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::geometry::{Offset, RRect, Radius, Rect, Size};
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, TextContext, TextLayoutError, paint::Paint};
use flui_protocol::build::{BuildState, Diagnostic};

use crate::performance_overlay::draw_label;

/// The gap between the banner and the surface's edges, and between it and the performance
/// overlay, in logical pixels.
const MARGIN: f64 = 8.0;
/// The inset of the banner's labels from its edges.
const PADDING: f64 = 8.0;
/// The step between the panel's lines.
const ROW: f64 = 16.0;
/// The width budgeted for one character of a panel line: a line is cut, with an ellipsis, to as
/// many characters as the panel's width holds at this advance. The clip cuts a line whose glyphs
/// run wider.
const CHARACTER_WIDTH: f64 = 7.0;
/// The chip's extent.
const CHIP_WIDTH: f64 = 96.0;
const CHIP_HEIGHT: f64 = 24.0;
/// The most characters a report keeps of a diagnostic's message, and of its file's name, the
/// ellipsis included: each window keeps its own copy of the report, and the widest panel holds
/// fewer on a line.
const FIELD_LIMIT: usize = 400;

const CHIP_BACKGROUND: Color = Color::rgba(10, 10, 15, 200);
const CHIP_TEXT: Color = Color::rgba(255, 220, 120, 255);
const PANEL_BACKGROUND: Color = Color::rgba(48, 8, 8, 230);
const HEADER_TEXT: Color = Color::rgba(255, 140, 140, 255);
const LINE_TEXT: Color = Color::rgba(235, 235, 235, 255);

/// What the chip says while a build runs.
pub(crate) const BUILDING: &str = "Building\u{2026}";
/// The panel's header for a failure that carries no diagnostic, as a manifest or dependency
/// resolution error does: the terminal that ran the build shows why.
pub(crate) const SEE_THE_TERMINAL: &str = "Build failed: see the terminal";

/// The report a presentation shows.
///
/// A report replaces the one before it and `Succeeded` clears it; nothing else does.
#[derive(Debug, Default)]
pub(crate) struct BuildBanner {
    shown: RefCell<Option<Report>>,
    /// The banner last laid out for the shown report, with the surface and the bounds it kept
    /// clear of: a frame for the same surface draws it again as it is. A report clears it.
    laid: RefCell<Option<Laid>>,
}

/// A build state this build of the runtime does not know, which [`BuildBanner::report`]
/// refuses. No report holds more diagnostics than a window shows:
/// [`Diagnostics`](flui_protocol::build::Diagnostics) holds that bound, decoded or built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnknownBuildState;

/// The report shown: a build running, or one that failed, as the panel reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Report {
    Building,
    /// The panel's header, then one line per diagnostic, its message and its file each
    /// [`one_line`] of at most [`FIELD_LIMIT`] characters.
    Failed {
        header: String,
        lines: Vec<String>,
    },
}

/// A banner laid out, and what it was laid out for.
#[derive(Debug)]
struct Laid {
    surface: Extent,
    avoid: Option<Rect<f64>>,
    banner: Option<Banner>,
}

impl BuildBanner {
    /// Shows `state`; whether what is shown changed.
    ///
    /// # Errors
    ///
    /// A report of a state this runtime does not know is refused and changes nothing.
    pub(crate) fn report(&self, state: BuildState) -> Result<bool, UnknownBuildState> {
        let report = match state {
            BuildState::Building => Some(Report::Building),
            BuildState::Succeeded => None,
            BuildState::Failed(errors) => Some(Report::Failed {
                header: header(errors.kept().len(), errors.omitted()),
                lines: errors.kept().iter().map(diagnostic_line).collect(),
            }),
            _ => return Err(UnknownBuildState),
        };
        if *self.shown.borrow() == report {
            return Ok(false);
        }
        // The replaced report and its layout are strings: their drops run no code of the
        // application's.
        drop(self.laid.take());
        drop(self.shown.replace(report));
        Ok(true)
    }

    /// The banner of the shown report on a surface of `surface` logical pixels, kept clear of
    /// `avoid` (the bounds of the performance overlay's readout while the scene shows it); `None`
    /// when nothing is shown or nothing of it fits. A frame for the surface the last one laid out
    /// for gets that banner again, without laying it out.
    pub(crate) fn banner(&self, surface: Size<f64>, avoid: Option<Rect<f64>>) -> Option<Banner> {
        let shown = self.shown.borrow();
        let report = shown.as_ref()?;
        let surface = Extent::of(surface);
        let mut laid = self.laid.borrow_mut();
        if let Some(laid) = laid
            .as_ref()
            .filter(|laid| laid.surface == surface && laid.avoid == avoid)
        {
            return laid.banner.clone();
        }
        let banner = match report {
            Report::Building => Banner::chip(surface, avoid),
            Report::Failed { header, lines } => Banner::panel(surface, avoid, header, lines),
        };
        // The layout replaced is strings: its drop runs no code of the application's.
        drop(laid.replace(Laid {
            surface,
            avoid,
            banner: banner.clone(),
        }));
        banner
    }
}

/// A surface's logical extent, finite and non-negative: an unbounded or non-finite extent is no
/// surface.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Extent {
    width: f64,
    height: f64,
}

impl Extent {
    fn of(size: Size<f64>) -> Self {
        let extent = |logical: f64| {
            if logical.is_finite() {
                logical.max(0.0)
            } else {
                0.0
            }
        };
        Self {
            width: extent(size.width),
            height: extent(size.height),
        }
    }
}

/// What one frame draws of the shown report: its bounds and background, and each label's text,
/// place, size and colour.
///
/// Two frames whose banners compare equal, shaped over the same fonts, record the same commands,
/// so the presentation keeps the chunk it published and the banner damages nothing while it
/// holds still. A clone shares its labels, and two banners that share them compare equal without
/// reading them.
#[derive(Debug, Clone)]
pub(crate) struct Banner {
    bounds: Rect<f64>,
    background: Color,
    labels: Rc<[Label]>,
}

impl PartialEq for Banner {
    fn eq(&self, other: &Self) -> bool {
        self.bounds == other.bounds
            && self.background == other.background
            && (Rc::ptr_eq(&self.labels, &other.labels) || self.labels == other.labels)
    }
}

/// One label of a [`Banner`], at its top-left corner on the surface, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
struct Label {
    text: String,
    at: Offset<f64>,
    font_size: f32,
    color: Color,
}

impl Banner {
    /// The chip that says a build runs: in the top-right corner, below `avoid` when it would
    /// overlap it there. It is drawn whole or not at all: a surface without room for it shows
    /// none.
    fn chip(surface: Extent, avoid: Option<Rect<f64>>) -> Option<Self> {
        let right = surface.width - MARGIN;
        let left = right - CHIP_WIDTH;
        let corner = Rect::from_ltrb(left, MARGIN, right, MARGIN + CHIP_HEIGHT);
        let top = avoid
            .filter(|avoid| corner.overlaps(avoid))
            .map_or(MARGIN, |avoid| avoid.bottom() + MARGIN);
        if left < 0.0 || top + CHIP_HEIGHT > surface.height {
            return None;
        }
        let bounds = Rect::from_ltrb(left, top, right, top + CHIP_HEIGHT);
        Some(Self {
            bounds,
            background: CHIP_BACKGROUND,
            labels: Rc::new([Label {
                text: BUILDING.to_owned(),
                at: Offset::new(left + PADDING, top + 4.0),
                font_size: 12.0,
                color: CHIP_TEXT,
            }]),
        })
    }

    /// The panel that says a build failed: along the bottom of the surface, below `avoid` when
    /// the two share columns. Its first line is `header`, and each of `lines` takes a line after
    /// it, as many as the height left holds, each cut to the width.
    fn panel(
        surface: Extent,
        avoid: Option<Rect<f64>>,
        header: &str,
        lines: &[String],
    ) -> Option<Self> {
        let left = MARGIN;
        let right = surface.width - MARGIN;
        if right - left < 2.0 * PADDING + CHARACTER_WIDTH {
            // Not one character fits across.
            return None;
        }
        let bottom = surface.height - MARGIN;
        let ceiling = avoid
            .filter(|avoid| avoid.left() < right && avoid.right() > left)
            .map_or(MARGIN, |avoid| avoid.bottom() + MARGIN);
        let room = bottom - ceiling - 2.0 * PADDING;
        let rows = if room >= ROW {
            (room / ROW).floor() as usize
        } else {
            0
        };
        // The header first, then as many diagnostics as fit after it.
        let shown = match rows {
            0 => return None,
            rows => lines.len().min(rows - 1),
        };
        let count = 1 + shown;
        let top = bottom - 2.0 * PADDING - count as f64 * ROW;
        let bounds = Rect::from_ltrb(left, top, right, bottom);
        if bounds.is_empty() {
            return None;
        }
        let budget = ((right - left - 2.0 * PADDING) / CHARACTER_WIDTH).max(0.0) as usize;
        let x = left + PADDING;
        let mut y = top + PADDING;
        let mut labels = Vec::with_capacity(count);
        let mut line = |text: &str, font_size: f32, color: Color| {
            labels.push(Label {
                text: fit(text, budget),
                at: Offset::new(x, y),
                font_size,
                color,
            });
            y += ROW;
        };
        line(header, 13.0, HEADER_TEXT);
        for text in &lines[..shown] {
            line(text, 12.0, LINE_TEXT);
        }
        Some(Self {
            bounds,
            background: PANEL_BACKGROUND,
            labels: Rc::from(labels),
        })
    }

    /// Where the banner draws, in logical pixels from the top-left corner of the surface: what it
    /// records stays inside, and the clip over it cuts what does not.
    pub(crate) fn bounds(&self) -> Rect<f64> {
        self.bounds
    }

    /// The banner recorded over its background, each label shaped through `text`.
    ///
    /// # Errors
    ///
    /// The [`TextLayoutError`] of the first label that does not shape.
    pub(crate) fn record(&self, text: &mut TextContext) -> Result<DisplayList, TextLayoutError> {
        let mut canvas = Canvas::new();
        canvas.draw_rrect(
            RRect::from_rect_and_radius(self.bounds, Radius::circular(4.0)),
            &Paint::fill(self.background),
        );
        for label in self.labels.iter() {
            draw_label(
                &mut canvas,
                text,
                &label.text,
                label.at,
                label.font_size,
                label.color,
            )?;
        }
        Ok(canvas.finish())
    }

    /// The text of each label, in drawing order.
    #[cfg(test)]
    pub(crate) fn texts(&self) -> Vec<&str> {
        self.labels
            .iter()
            .map(|label| label.text.as_str())
            .collect()
    }

    /// A chip that never records: its first label shapes, so each attempt to record it shapes
    /// that label unless the context holds its layout, and its second has a font size no shaper
    /// takes.
    #[cfg(test)]
    pub(crate) fn unshapeable() -> Self {
        let label = |at: f64, font_size: f32| Label {
            text: BUILDING.to_owned(),
            at: Offset::new(16.0, at),
            font_size,
            color: CHIP_TEXT,
        };
        Self {
            bounds: Rect::from_ltrb(8.0, 8.0, 104.0, 56.0),
            background: CHIP_BACKGROUND,
            labels: Rc::new([label(12.0, 12.0), label(32.0, f32::NAN)]),
        }
    }
}

/// The panel's first line: how many errors the report carries, and how many it left out; a
/// failure that carries none sends the reader to the terminal.
fn header(diagnostics: usize, omitted: u32) -> String {
    let errors = |one: bool| if one { "error" } else { "errors" };
    match (diagnostics, omitted) {
        (0, 0) => SEE_THE_TERMINAL.to_owned(),
        (0, omitted) => format!(
            "Build failed: {omitted} {}, see the terminal",
            errors(omitted == 1)
        ),
        (diagnostics, 0) => format!("Build failed: {diagnostics} {}", errors(diagnostics == 1)),
        (diagnostics, omitted) => format!(
            "Build failed: {diagnostics} {} and {omitted} more",
            errors(diagnostics == 1)
        ),
    }
}

/// `file:line:column  message`, or the message alone where it has no location, its file and its
/// message each [`one_line`] of at most [`FIELD_LIMIT`] characters.
fn diagnostic_line(diagnostic: &Diagnostic) -> String {
    let message = one_line(&diagnostic.message, FIELD_LIMIT);
    match &diagnostic.location {
        Some(location) => format!(
            "{}:{}:{}  {message}",
            one_line(&location.file, FIELD_LIMIT),
            location.line,
            location.column
        ),
        None => message,
    }
}

/// Whether `character` is a bidirectional formatting character: an embedding, an override, an
/// isolate or an implicit mark. Each reorders the characters around it, so a line that carries
/// one can show other text than it holds.
fn is_bidi_format(character: char) -> bool {
    matches!(
        character,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

/// The characters Unicode makes a mandatory line break (UAX #14's `BK`, `CR`, `LF` and `NL`):
/// the shaper breaks a line at each, so a banner line holds none.
const LINE_BREAKS: [char; 7] = [
    '\n', '\r', '\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}',
];

/// `text` as one line of at most `limit` characters, the ellipsis included: cut at its first
/// [line break](LINE_BREAKS) when more than whitespace follows it, each other control character a
/// space, every bidirectional formatting character dropped, and ended with an ellipsis where it is
/// cut. It copies no more of `text` than the line it keeps.
fn one_line(text: &str, limit: usize) -> String {
    let (first, rest) = text
        .find(LINE_BREAKS)
        .map_or((text, ""), |at| text.split_at(at));
    let mut cut = !rest.trim().is_empty();
    let mut kept = String::new();
    let mut count = 0;
    for character in first.chars() {
        if is_bidi_format(character) {
            continue;
        }
        if count == limit {
            cut = true;
            break;
        }
        kept.push(if character.is_control() {
            ' '
        } else {
            character
        });
        count += 1;
    }
    if cut && limit > 0 {
        if count == limit {
            kept.pop();
        }
        kept.push('\u{2026}');
    }
    kept
}

/// `text` in at most `budget` characters: whole when it fits, otherwise its first `budget - 1`
/// and an ellipsis.
fn fit(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    match text.char_indices().nth(budget - 1) {
        Some((last, _)) if text.char_indices().nth(budget).is_some() => {
            format!("{}\u{2026}", &text[..last])
        }
        _ => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::Size;
    use flui_protocol::build::{BuildState, Diagnostic, MAX_DIAGNOSTICS, SourceLocation};

    use super::{BuildBanner, FIELD_LIMIT, Report, fit, one_line};
    use crate::performance_overlay::BOUNDS;

    /// A failed build as a window may read it off the wire: `diagnostics`, and `omitted` more.
    fn failed_with(diagnostics: &[Diagnostic], omitted: u32) -> BuildState {
        serde_json::from_value::<BuildState>(serde_json::json!({
            "failed": { "diagnostics": diagnostics, "omitted": omitted }
        }))
        .expect("a report within the bound")
    }

    fn failed(count: usize, omitted: u32) -> BuildState {
        let diagnostics: Vec<Diagnostic> = (0..count)
            .map(|index| {
                Diagnostic::new(
                    format!("error {index}"),
                    Some(SourceLocation::new("src/main.rs".to_owned(), 3, 7)),
                )
            })
            .collect();
        failed_with(&diagnostics, omitted)
    }

    /// A report's text is one line at its limit, with an ellipsis where it is cut at a line break
    /// or at the limit, every other control character a space and every bidirectional formatting
    /// character dropped; one that fits is kept whole. Fails if a line break, an override or an
    /// isolate reaches the panel, or a cut is not marked.
    #[test]
    fn a_report_line_is_one_line_without_bidi_formatting() {
        for (text, limit, expected) in [
            ("short", 10, "short"),
            ("exactly10!", 10, "exactly10!"),
            ("eleven char", 10, "eleven ch\u{2026}"),
            ("first\nsecond", 20, "first\u{2026}"),
            ("first\r\nsecond", 20, "first\u{2026}"),
            ("trailing\n", 20, "trailing"),
            ("first\u{2028}second", 20, "first\u{2026}"),
            ("first\u{2029}second", 20, "first\u{2026}"),
            ("first\u{0085}second", 20, "first\u{2026}"),
            ("first\u{000B}second", 20, "first\u{2026}"),
            ("first\u{000C}second", 20, "first\u{2026}"),
            ("trailing\u{2029}", 20, "trailing"),
            ("tab\there", 20, "tab here"),
            ("a\u{202E}cba\u{202C}d", 20, "acbad"),
            (
                "\u{2066}x\u{2069}\u{2067}y\u{2068}\u{200E}\u{200F}\u{061C}z\u{202A}\u{202B}\u{202D}",
                20,
                "xyz",
            ),
            ("\u{202E}0123456789", 10, "0123456789"),
            ("anything", 0, ""),
            ("", 4, ""),
        ] {
            assert_eq!(one_line(text, limit), expected, "{text:?} at {limit}");
        }
    }

    /// A line is cut to the panel's budget with an ellipsis; one that fits is kept whole.
    #[test]
    fn a_line_is_cut_to_its_budget() {
        for (text, budget, expected) in [
            ("short", 10, "short"),
            ("exactly10!", 10, "exactly10!"),
            ("eleven char", 10, "eleven ch\u{2026}"),
            ("\u{e9}\u{e9}\u{e9}\u{e9}", 3, "\u{e9}\u{e9}\u{2026}"),
            ("anything", 0, ""),
            ("", 4, ""),
        ] {
            assert_eq!(fit(text, budget), expected, "{text:?} at {budget}");
        }
    }

    /// A diagnostic's message and file are each bounded when the report is taken, whatever their
    /// length on the wire. Fails if a window keeps more of either than the limit.
    #[test]
    fn a_report_keeps_a_bounded_copy_of_each_diagnostic() {
        let long = "m".repeat(100_000);
        let banner = BuildBanner::default();
        assert_eq!(
            banner.report(failed_with(
                &[
                    Diagnostic::new(long.clone(), None),
                    Diagnostic::new("short".to_owned(), Some(SourceLocation::new(long, 1, 2))),
                ],
                0
            )),
            Ok(true)
        );
        let shown = banner.shown.borrow();
        let Some(Report::Failed { lines, .. }) = shown.as_ref() else {
            panic!("a failure is shown: {shown:?}");
        };
        assert_eq!(lines[0].chars().count(), FIELD_LIMIT);
        assert!(lines[0].ends_with('\u{2026}'), "the cut is marked");
        assert_eq!(
            lines[1].chars().count(),
            FIELD_LIMIT + ":1:2  short".chars().count()
        );
    }

    /// The panel counts the errors and what was left out and draws one line per diagnostic as
    /// `file:line:column  message`; a failure with no diagnostic sends the reader to the terminal
    /// instead of counting none.
    #[test]
    fn the_failure_panel_reads_header_and_diagnostics() {
        let banner = BuildBanner::default();
        let drawn = |state: BuildState| {
            assert_eq!(banner.report(state), Ok(true));
            banner
                .banner(Size::new(800.0, 600.0), None)
                .expect("a failure is drawn")
                .texts()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<String>>()
        };
        assert_eq!(
            drawn(failed(2, 5)),
            [
                "Build failed: 2 errors and 5 more",
                "src/main.rs:3:7  error 0",
                "src/main.rs:3:7  error 1",
            ]
        );
        assert_eq!(
            drawn(failed(1, 0)),
            ["Build failed: 1 error", "src/main.rs:3:7  error 0"]
        );
        assert_eq!(drawn(failed(0, 0)), [super::SEE_THE_TERMINAL]);
        assert_eq!(
            drawn(failed(0, 3)),
            ["Build failed: 3 errors, see the terminal"]
        );
        assert_eq!(
            drawn(failed_with(
                &[Diagnostic::new("no location".to_owned(), None)],
                1
            )),
            ["Build failed: 1 error and 1 more", "no location"]
        );
    }

    /// A report replaces the one shown, an equal report changes nothing, `Succeeded` clears it.
    #[test]
    fn a_report_replaces_or_clears() {
        let banner = BuildBanner::default();
        assert!(banner.shown.borrow().is_none());
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        assert_eq!(banner.report(BuildState::Building), Ok(false));
        let size = Size::new(800.0, 600.0);
        let building = banner.banner(size, None).expect("the chip shows");
        assert_eq!(building.texts(), [super::BUILDING]);
        assert_eq!(banner.report(failed(MAX_DIAGNOSTICS, 0)), Ok(true));
        let panel = banner
            .banner(size, None)
            .expect("the failure replaces the chip");
        assert_eq!(panel.texts().len(), MAX_DIAGNOSTICS + 1);
        assert_eq!(banner.report(BuildState::Succeeded), Ok(true));
        assert!(banner.shown.borrow().is_none());
        assert_eq!(banner.banner(size, None), None);
        assert_eq!(banner.report(BuildState::Succeeded), Ok(false));
    }

    /// A frame for the surface the last one laid out for gets the same banner without a layout,
    /// and one for another surface or another area to avoid gets it laid out anew. Fails if a
    /// resize, or the performance overlay turned on, keeps a stale layout.
    #[test]
    fn a_layout_is_kept_for_its_surface() {
        let banner = BuildBanner::default();
        assert_eq!(banner.report(failed(1, 0)), Ok(true));
        let wide = Size::new(800.0, 600.0);
        let first = banner.banner(wide, None).expect("a failure is drawn");
        let again = banner.banner(wide, None).expect("a failure is drawn");
        assert!(
            std::rc::Rc::ptr_eq(&first.labels, &again.labels),
            "the layout is kept"
        );
        let low = banner
            .banner(Size::new(800.0, 300.0), None)
            .expect("a failure is drawn");
        assert_ne!(first.bounds(), low.bounds(), "a resize lays it out anew");
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        let building = banner.banner(wide, None).expect("the chip is drawn");
        let avoiding = banner
            .banner(Size::new(500.0, 600.0), Some(BOUNDS))
            .expect("the chip is drawn");
        let plain = banner
            .banner(Size::new(500.0, 600.0), None)
            .expect("the chip is drawn");
        assert_ne!(building.bounds(), plain.bounds(), "a resize moves the chip");
        assert_ne!(avoiding.bounds(), plain.bounds(), "the readout moves it");
        assert!(!avoiding.bounds().overlaps(&BOUNDS));
    }

    /// For any surface, zero, tiny, huge or not finite, every banner is finite, non-negative,
    /// inside the surface, clear of the performance overlay and every label inside it. Each
    /// report is drawn on every finite surface of at least 600 by 200 logical pixels, and on none
    /// narrower than 39 or lower than 32. Fails if a banner that has room draws nothing.
    #[test]
    fn a_banner_is_finite_inside_its_surface_and_clear_of_the_performance_overlay() {
        let extents = [
            0.0,
            1.0,
            7.9,
            8.0,
            16.0,
            40.0,
            66.0,
            100.0,
            333.3,
            488.0,
            1920.0,
            1.0e9,
            f64::INFINITY,
            f64::NAN,
            -5.0,
        ];
        let finite_at_least = |extent: f64, least: f64| extent.is_finite() && extent >= least;
        for state in [
            BuildState::Building,
            failed(MAX_DIAGNOSTICS, 3),
            failed(0, 0),
        ] {
            let banner = BuildBanner::default();
            assert_eq!(banner.report(state.clone()), Ok(true));
            let mut drawn_count = 0;
            for width in extents {
                for height in extents {
                    for avoid in [None, Some(BOUNDS)] {
                        let case = format!("{state:?} on {width}x{height}, avoiding {avoid:?}");
                        let drawn = banner.banner(Size::new(width, height), avoid);
                        if finite_at_least(width, 600.0) && finite_at_least(height, 200.0) {
                            assert!(drawn.is_some(), "{case}: it has room");
                        }
                        if !finite_at_least(width, 39.0) || !finite_at_least(height, 32.0) {
                            assert!(drawn.is_none(), "{case}: nothing fits");
                        }
                        let Some(drawn) = drawn else {
                            continue;
                        };
                        drawn_count += 1;
                        let bounds = drawn.bounds();
                        assert!(
                            bounds.is_finite() && !bounds.is_empty(),
                            "{case}: {bounds:?}"
                        );
                        assert!(bounds.left() >= 0.0 && bounds.top() >= 0.0, "{case}");
                        assert!(
                            bounds.right() <= width && bounds.bottom() <= height,
                            "{case}: {bounds:?}"
                        );
                        if let Some(avoid) = avoid {
                            assert!(!bounds.overlaps(&avoid), "{case}: {bounds:?}");
                        }
                        for label in drawn.labels.iter() {
                            assert!(
                                label.at.dx.is_finite()
                                    && label.at.dy.is_finite()
                                    && bounds.contains_offset(label.at),
                                "{case}: {label:?} in {bounds:?}"
                            );
                        }
                    }
                }
            }
            // At least 1920 and 1e9 on each axis, with and without the readout to avoid.
            assert!(drawn_count >= 8, "{state:?} drew {drawn_count} times");
        }
    }

    /// A panel on a short surface keeps its header and drops the diagnostics that do not fit; on
    /// a large one it shows each.
    #[test]
    fn a_short_panel_keeps_its_header() {
        let banner = BuildBanner::default();
        assert_eq!(banner.report(failed(MAX_DIAGNOSTICS, 0)), Ok(true));
        let tall = banner
            .banner(Size::new(800.0, 800.0), None)
            .expect("a failure is drawn");
        assert_eq!(tall.texts().len(), MAX_DIAGNOSTICS + 1);
        let short = banner
            .banner(Size::new(800.0, 100.0), None)
            .expect("a failure is drawn");
        let texts = short.texts();
        assert!((2..=MAX_DIAGNOSTICS).contains(&texts.len()), "{texts:?}");
        assert!(texts[0].starts_with("Build failed"), "{texts:?}");
        let header_only = banner
            .banner(Size::new(800.0, 56.0), None)
            .expect("the header fits");
        assert_eq!(header_only.texts().len(), 1);
    }
}
