//! The build banner a presentation draws over its scene while a development tool reports a
//! build of the application (`build.report`, ADR-0288 §8): a chip in the top-right corner while
//! a build runs, a panel along the bottom with the compiler's errors when it failed.
//!
//! The report reaches the presentation's [`BuildBanner`] through the agent's annotate path
//! (`ui_runtime::agent_serve`), which hands a handler this slot and nothing else of the
//! application. A frame lays the report out for the surface it draws ([`BuildBanner::banner`]),
//! and the presentation publishes the result as a node of its own after the performance
//! overlay's, recorded as that readout is: its labels shaped through the UI runtime's
//! [`TextContext`], a clip to its bounds, under the device-pixel-ratio scale
//! (`presentation_scene`). The application never sees it: it is not in the render, semantics or
//! hit-test trees, and a press on it is taken before the gesture binding
//! ([`BuildBanner::consumes`]).
//!
//! Owner thread only (ADR-0323 §1): the slot is cells.

use std::cell::{Cell, RefCell};

use flui_foundation::geometry::{Offset, RRect, Radius, Rect, Size};
use flui_interaction::events::{PointerEvent, PointerEventExt as _};
use flui_interaction::ids::InputSurface;
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, TextContext, TextLayoutError, paint::Paint};
use flui_platform::input::pointer::PointerId;
use flui_protocol::build::{BuildState, Diagnostic, MAX_DIAGNOSTICS};

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
/// How many pointers a dismissal follows to their end at once. Past it the oldest is let go:
/// the rest of its contact reaches the application, which saw none of its start and so treats it
/// as a pointer it holds nothing for.
const MAX_FOLLOWED_POINTERS: usize = 8;

const CHIP_BACKGROUND: Color = Color::rgba(10, 10, 15, 200);
const CHIP_TEXT: Color = Color::rgba(255, 220, 120, 255);
const PANEL_BACKGROUND: Color = Color::rgba(48, 8, 8, 230);
const HEADER_TEXT: Color = Color::rgba(255, 140, 140, 255);
const LINE_TEXT: Color = Color::rgba(235, 235, 235, 255);
const HINT_TEXT: Color = Color::rgba(160, 160, 160, 255);

/// What the chip says while a build runs.
pub(crate) const BUILDING: &str = "Building\u{2026}";
/// The panel's last line.
pub(crate) const DISMISS_HINT: &str = "Tap to dismiss";

/// The report a presentation shows, and the pointers a press on it took from the application.
///
/// A report replaces the one before it; `Succeeded` clears it, and so does a press inside the
/// banner a frame drew for it. Only what the last frame drew can be pressed: until a frame draws
/// a new report, a press goes to the application.
#[derive(Debug, Default)]
pub(crate) struct BuildBanner {
    shown: RefCell<Option<Report>>,
    /// The bounds the last frame drew the shown report at; `None` while nothing of it is drawn.
    drawn: Cell<Option<Rect<f64>>>,
    /// The pointers whose press dismissed the banner: the rest of each contact, to its up or
    /// cancel, is the banner's too.
    followed: RefCell<Vec<PointerId>>,
}

/// What [`BuildBanner::report`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportRefused {
    /// More diagnostics than [`MAX_DIAGNOSTICS`].
    TooManyDiagnostics,
    /// A build state this build of the runtime does not know.
    UnknownState,
}

/// What [`BuildBanner::consumes`] made of a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Consumption {
    /// It is the application's.
    Passes,
    /// It continues a contact whose press dismissed the banner.
    Taken,
    /// It pressed the banner, which is dismissed: the presentation owes a frame without it.
    Dismissed,
}

/// The report shown: a build running, or one that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Report {
    Building,
    Failed {
        diagnostics: Vec<Diagnostic>,
        omitted: u32,
    },
}

impl BuildBanner {
    /// Shows `state`; whether what is shown changed.
    ///
    /// # Errors
    ///
    /// A report with more than [`MAX_DIAGNOSTICS`] diagnostics, or of a state this runtime does
    /// not know, is refused and changes nothing.
    pub(crate) fn report(&self, state: BuildState) -> Result<bool, ReportRefused> {
        let report = match state {
            BuildState::Building => Some(Report::Building),
            BuildState::Succeeded => None,
            BuildState::Failed {
                diagnostics,
                omitted,
            } => {
                if diagnostics.len() > MAX_DIAGNOSTICS {
                    return Err(ReportRefused::TooManyDiagnostics);
                }
                Some(Report::Failed {
                    diagnostics,
                    omitted,
                })
            }
            _ => return Err(ReportRefused::UnknownState),
        };
        if *self.shown.borrow() == report {
            return Ok(false);
        }
        // What was drawn is of the report replaced: nothing of the new one can be pressed
        // before a frame draws it.
        self.drawn.set(None);
        // The replaced report is strings: its drop runs no code of the application's.
        drop(self.shown.replace(report));
        Ok(true)
    }

    /// The banner of the shown report on a surface of `surface` logical pixels, kept clear of
    /// `avoid` (the performance overlay's bounds while it is drawn); `None` when nothing is shown
    /// or nothing of it fits.
    pub(crate) fn banner(&self, surface: Size<f64>, avoid: Option<Rect<f64>>) -> Option<Banner> {
        let shown = self.shown.borrow();
        let surface = Extent::of(surface);
        match shown.as_ref()? {
            Report::Building => Banner::chip(surface, avoid),
            Report::Failed {
                diagnostics,
                omitted,
            } => Banner::panel(surface, avoid, diagnostics, *omitted),
        }
    }

    /// The frame drew the banner at `bounds`, or drew none.
    pub(crate) fn set_drawn(&self, bounds: Option<Rect<f64>>) {
        self.drawn
            .set(bounds.filter(|_| self.shown.borrow().is_some()));
    }

    /// Whether `event`, on `surface` of the presentation's window, is the banner's: a press
    /// inside what the last frame drew dismisses it, and the rest of that contact is the
    /// banner's too, to its up or cancel. Everything else passes to the application.
    ///
    /// Only the window's own surface is drawn over; an embedded surface's input passes.
    pub(crate) fn consumes(&self, surface: InputSurface, event: &PointerEvent) -> Consumption {
        if surface != InputSurface::Window {
            return Consumption::Passes;
        }
        let Some(pointer) = event.pointer_id() else {
            return Consumption::Passes;
        };
        let pressed = matches!(event, PointerEvent::Down(_));
        let mut followed = self.followed.borrow_mut();
        if let Some(at) = followed.iter().position(|held| *held == pointer) {
            // A contact ends with its up or cancel. A press of a pointer still followed starts a
            // new contact (the end of the one before was lost, as on a platform that withdrew
            // it), which is judged afresh below.
            if pressed || matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) {
                followed.remove(at);
            }
            if !pressed {
                return Consumption::Taken;
            }
        }
        if !pressed {
            return Consumption::Passes;
        }
        let inside = event
            .position()
            .zip(self.drawn.get())
            .is_some_and(|(position, drawn)| contains(drawn, position));
        if !inside {
            return Consumption::Passes;
        }
        self.drawn.set(None);
        drop(self.shown.replace(None));
        if followed.len() == MAX_FOLLOWED_POINTERS {
            followed.remove(0);
        }
        followed.push(pointer);
        Consumption::Dismissed
    }
}

/// Whether `point` is inside `bounds`, its right and bottom edges excluded.
fn contains(bounds: Rect<f64>, point: Offset<f64>) -> bool {
    point.dx >= bounds.left()
        && point.dx < bounds.right()
        && point.dy >= bounds.top()
        && point.dy < bounds.bottom()
}

/// A surface's logical extent, finite and non-negative: an unbounded or non-finite extent is no
/// surface.
#[derive(Debug, Clone, Copy)]
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
/// holds still.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Banner {
    bounds: Rect<f64>,
    background: Color,
    labels: Vec<Label>,
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
            labels: vec![Label {
                text: BUILDING.to_owned(),
                at: Offset::new(left + PADDING, top + 4.0),
                font_size: 12.0,
                color: CHIP_TEXT,
            }],
        })
    }

    /// The panel that says a build failed: along the bottom of the surface, below `avoid` when
    /// the two share columns. Its first line counts the errors, its last says how to dismiss it,
    /// and each diagnostic takes a line between them, as many as the height left holds, each cut
    /// to the width.
    fn panel(
        surface: Extent,
        avoid: Option<Rect<f64>>,
        diagnostics: &[Diagnostic],
        omitted: u32,
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
        // The header first, the hint next, then as many diagnostics as fit between them.
        let (shown, hint) = match rows {
            0 => return None,
            1 => (0, false),
            rows => (diagnostics.len().min(rows - 2), true),
        };
        let lines = 1 + shown + usize::from(hint);
        let top = bottom - 2.0 * PADDING - lines as f64 * ROW;
        let bounds = Rect::from_ltrb(left, top, right, bottom);
        if bounds.is_empty() {
            return None;
        }
        let budget = ((right - left - 2.0 * PADDING) / CHARACTER_WIDTH).max(0.0) as usize;
        let x = left + PADDING;
        let mut y = top + PADDING;
        let mut labels = Vec::with_capacity(lines);
        let mut line = |text: String, font_size: f32, color: Color| {
            labels.push(Label {
                text: cut(&text, budget),
                at: Offset::new(x, y),
                font_size,
                color,
            });
            y += ROW;
        };
        line(header(diagnostics.len(), omitted), 13.0, HEADER_TEXT);
        for diagnostic in &diagnostics[..shown] {
            line(diagnostic_line(diagnostic), 12.0, LINE_TEXT);
        }
        if hint {
            line(DISMISS_HINT.to_owned(), 11.0, HINT_TEXT);
        }
        Some(Self {
            bounds,
            background: PANEL_BACKGROUND,
            labels,
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
        for label in &self.labels {
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
}

/// The panel's first line: how many errors the report carries, and how many it left out.
fn header(diagnostics: usize, omitted: u32) -> String {
    let errors = if diagnostics == 1 { "error" } else { "errors" };
    if omitted == 0 {
        format!("Build failed: {diagnostics} {errors}")
    } else {
        format!("Build failed: {diagnostics} {errors} and {omitted} more")
    }
}

/// `file:line:column  message`, or the message alone where it has no location.
fn diagnostic_line(diagnostic: &Diagnostic) -> String {
    match &diagnostic.location {
        Some(location) => format!(
            "{}:{}:{}  {}",
            location.file, location.line, location.column, diagnostic.message
        ),
        None => diagnostic.message.clone(),
    }
}

/// `text` as one line of at most `budget` characters: cut at its first line break, each other
/// control character a space, and ended with an ellipsis where it is cut.
fn cut(text: &str, budget: usize) -> String {
    let first = text.split(['\n', '\r']).next().unwrap_or_default();
    let mut cut = first.len() < text.len();
    let mut kept = String::new();
    let mut count = 0;
    for character in first.chars() {
        if count == budget {
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
    if cut && budget > 0 {
        if count == budget {
            kept.pop();
        }
        kept.push('\u{2026}');
    }
    kept
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::Size;
    use flui_protocol::build::{BuildState, Diagnostic, MAX_DIAGNOSTICS, SourceLocation};

    use super::{BuildBanner, ReportRefused, cut};
    use crate::performance_overlay::BOUNDS;

    fn failed(count: usize, omitted: u32) -> BuildState {
        BuildState::Failed {
            diagnostics: (0..count)
                .map(|index| {
                    Diagnostic::new(
                        format!("error {index}"),
                        Some(SourceLocation::new("src/main.rs".to_owned(), 3, 7)),
                    )
                })
                .collect(),
            omitted,
        }
    }

    /// A line is cut to its budget with an ellipsis, at its first line break, with every other
    /// control character a space; one that fits is kept whole.
    #[test]
    fn a_line_is_cut_to_one_line_at_its_budget() {
        for (text, budget, expected) in [
            ("short", 10, "short"),
            ("exactly10!", 10, "exactly10!"),
            ("eleven char", 10, "eleven ch\u{2026}"),
            ("first\nsecond", 20, "first\u{2026}"),
            ("tab\there", 20, "tab here"),
            ("anything", 0, ""),
            ("", 4, ""),
        ] {
            assert_eq!(cut(text, budget), expected, "{text:?} at {budget}");
        }
    }

    /// The panel counts the errors and what was left out, draws one line per diagnostic as
    /// `file:line:column  message`, and ends with the hint.
    #[test]
    fn the_failure_panel_reads_header_diagnostics_and_hint() {
        let banner = BuildBanner::default();
        assert_eq!(banner.report(failed(2, 5)), Ok(true));
        let drawn = banner
            .banner(Size::new(800.0, 600.0), None)
            .expect("a failure is drawn");
        assert_eq!(
            drawn.texts(),
            [
                "Build failed: 2 errors and 5 more",
                "src/main.rs:3:7  error 0",
                "src/main.rs:3:7  error 1",
                super::DISMISS_HINT,
            ]
        );
        assert_eq!(banner.report(failed(1, 0)), Ok(true));
        let drawn = banner
            .banner(Size::new(800.0, 600.0), None)
            .expect("a failure is drawn");
        assert_eq!(drawn.texts()[0], "Build failed: 1 error");
    }

    /// A report past the limit is refused and keeps what was shown; an equal report changes
    /// nothing; `Succeeded` clears it.
    #[test]
    fn a_report_replaces_refuses_or_clears() {
        let banner = BuildBanner::default();
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        assert_eq!(banner.report(BuildState::Building), Ok(false));
        assert_eq!(
            banner.report(failed(MAX_DIAGNOSTICS + 1, 0)),
            Err(ReportRefused::TooManyDiagnostics)
        );
        let size = Size::new(800.0, 600.0);
        let building = banner.banner(size, None).expect("the chip stays");
        assert_eq!(building.texts(), [super::BUILDING]);
        assert_eq!(banner.report(failed(MAX_DIAGNOSTICS, 0)), Ok(true));
        assert_eq!(banner.report(BuildState::Succeeded), Ok(true));
        assert_eq!(banner.banner(size, None), None);
        assert_eq!(banner.report(BuildState::Succeeded), Ok(false));
    }

    /// For any surface, zero, tiny, huge or not finite, every banner is finite, non-negative,
    /// inside the surface, clear of the performance overlay and every label inside it.
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
        for state in [
            BuildState::Building,
            failed(MAX_DIAGNOSTICS, 3),
            failed(0, 0),
        ] {
            let banner = BuildBanner::default();
            assert_eq!(banner.report(state.clone()), Ok(true));
            for width in extents {
                for height in extents {
                    for avoid in [None, Some(BOUNDS)] {
                        let size = Size::new(width, height);
                        let Some(drawn) = banner.banner(size, avoid) else {
                            continue;
                        };
                        let bounds = drawn.bounds();
                        let case = format!("{state:?} on {width}x{height}, avoiding {avoid:?}");
                        assert!(
                            bounds.is_finite() && !bounds.is_empty(),
                            "{case}: {bounds:?}"
                        );
                        assert!(bounds.left() >= 0.0 && bounds.top() >= 0.0, "{case}");
                        let surface_width = if width.is_finite() { width } else { 0.0 };
                        let surface_height = if height.is_finite() { height } else { 0.0 };
                        assert!(
                            bounds.right() <= surface_width && bounds.bottom() <= surface_height,
                            "{case}: {bounds:?}"
                        );
                        if let Some(avoid) = avoid {
                            assert!(!bounds.overlaps(&avoid), "{case}: {bounds:?}");
                        }
                        for label in &drawn.labels {
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
        }
    }

    /// A press inside what was drawn dismisses the banner, and the same contact's moves and up
    /// are the banner's; once it ended, that pointer's input passes again, as does a press
    /// outside, on an embedded surface, or before a frame drew the report. Fails if the rest of
    /// a dismissing contact leaks to the application, or its end is never seen.
    #[test]
    fn a_dismissing_press_takes_its_contact_to_its_end() {
        use flui_interaction::events::{
            PointerKind, make_cancel_event_for_id, make_down_event, make_down_event_for_id,
            make_move_event, make_up_event,
        };
        use flui_interaction::ids::InputSurface;

        use super::Consumption;

        let at = |x: f64, y: f64| flui_foundation::geometry::Offset::new(x, y);
        let kind = PointerKind::Touch;
        let banner = BuildBanner::default();
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        let inside = make_down_event(at(20.0, 20.0), kind).expect("finite");
        assert_eq!(
            banner.consumes(InputSurface::Window, &inside),
            Consumption::Passes,
            "nothing is drawn yet"
        );
        banner.set_drawn(Some(flui_foundation::geometry::Rect::from_ltrb(
            10.0, 10.0, 50.0, 30.0,
        )));
        let outside = make_down_event(at(60.0, 20.0), kind).expect("finite");
        assert_eq!(
            banner.consumes(InputSurface::Window, &outside),
            Consumption::Passes
        );
        assert_eq!(
            banner.consumes(InputSurface::Window, &inside),
            Consumption::Dismissed
        );
        assert_eq!(banner.banner(Size::new(800.0, 600.0), None), None);
        let moved = make_move_event(at(70.0, 70.0), kind).expect("finite");
        assert_eq!(
            banner.consumes(InputSurface::Window, &moved),
            Consumption::Taken
        );
        let up = make_up_event(at(70.0, 70.0), kind).expect("finite");
        assert_eq!(
            banner.consumes(InputSurface::Window, &up),
            Consumption::Taken
        );
        assert_eq!(
            banner.consumes(InputSurface::Window, &moved),
            Consumption::Passes,
            "the contact ended with its up"
        );

        // A cancel ends a contact as an up does; an embedded surface is not drawn over.
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        banner.set_drawn(Some(flui_foundation::geometry::Rect::from_ltrb(
            10.0, 10.0, 50.0, 30.0,
        )));
        let other = flui_platform::input::pointer::PointerId::new(
            std::num::NonZeroU64::new(7).expect("non-zero"),
        );
        let press = make_down_event_for_id(other, at(20.0, 20.0), kind).expect("finite");
        let embedded = flui_interaction::binding::GestureBinding::new()
            .embed_surface()
            .expect("a surface identity");
        assert_eq!(banner.consumes(embedded, &press), Consumption::Passes);
        assert_eq!(
            banner.consumes(InputSurface::Window, &press),
            Consumption::Dismissed
        );
        let cancel = make_cancel_event_for_id(other, kind);
        assert_eq!(
            banner.consumes(InputSurface::Window, &cancel),
            Consumption::Taken
        );
        assert_eq!(
            banner.consumes(InputSurface::Window, &cancel),
            Consumption::Passes
        );

        // A press of a pointer still followed, its end lost, starts a contact the application
        // sees.
        assert_eq!(banner.report(BuildState::Building), Ok(true));
        banner.set_drawn(Some(flui_foundation::geometry::Rect::from_ltrb(
            10.0, 10.0, 50.0, 30.0,
        )));
        assert_eq!(
            banner.consumes(InputSurface::Window, &press),
            Consumption::Dismissed
        );
        let elsewhere = make_down_event_for_id(other, at(60.0, 20.0), kind).expect("finite");
        assert_eq!(
            banner.consumes(InputSurface::Window, &elsewhere),
            Consumption::Passes,
            "a new contact is judged afresh"
        );
        let after = make_cancel_event_for_id(other, kind);
        assert_eq!(
            banner.consumes(InputSurface::Window, &after),
            Consumption::Passes,
            "the new contact is the application's to its end"
        );
    }

    /// A panel on a short surface keeps its header and hint and drops the diagnostics that do
    /// not fit; on a large one it shows each.
    #[test]
    fn a_short_panel_keeps_its_header_and_hint() {
        let banner = BuildBanner::default();
        assert_eq!(banner.report(failed(MAX_DIAGNOSTICS, 0)), Ok(true));
        let tall = banner
            .banner(Size::new(800.0, 800.0), None)
            .expect("a failure is drawn");
        assert_eq!(tall.texts().len(), MAX_DIAGNOSTICS + 2);
        let short = banner
            .banner(Size::new(800.0, 100.0), None)
            .expect("a failure is drawn");
        let texts = short.texts();
        assert!(texts.len() < MAX_DIAGNOSTICS + 2, "{texts:?}");
        assert!(texts[0].starts_with("Build failed"), "{texts:?}");
        assert_eq!(texts.last(), Some(&super::DISMISS_HINT), "{texts:?}");
    }
}
