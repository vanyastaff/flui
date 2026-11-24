//! The frame-statistics readout a presentation draws over its scene while
//! its debug overlay is on (`UiRuntime::set_performance_overlay`).
//!
//! The readout is recorded here, its labels shaped through the UI runtime's
//! [`TextContext`] so the raster side only replays them (ADR-0092), and the
//! presentation publishes it as a node of its own: a clip to [`BOUNDS`] over
//! one chunk of the recording, under the device-pixel-ratio scale when there
//! is one (`presentation_scene`). Nothing here measures: the presentation
//! that owns the frame clock samples the numbers and hands them in.

use std::sync::Arc;

use flui_foundation::TextDirection;
use flui_foundation::geometry::{Offset, RRect, Radius, Rect};
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, TextContext, TextLayoutError, paint::Paint};

/// Where the readout is drawn, in logical pixels from the top-left corner:
/// what it records stays inside, and the clip over it cuts what does not.
pub(crate) const BOUNDS: Rect<f64> = Rect::from_ltrb(8.0, 8.0, 488.0, 66.0);

/// The numbers one frame's readout shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PerformanceSample<'a> {
    /// Frames per second.
    pub(crate) fps: f64,
    /// Average frame time in milliseconds.
    pub(crate) frame_time_ms: f64,
    /// One extra line of runtime diagnostics (present and input percentiles,
    /// dropped-frame counts), drawn as a third row when present.
    pub(crate) diagnostic_line: Option<&'a str>,
}

/// The overlay's background: a translucent near-black.
const BACKGROUND: Color = Color::rgba(10, 10, 15, 200);
/// The label column's inset from the left edge.
const LABEL_X: f64 = 8.0;
/// The value column's inset from the left edge.
const VALUE_X: f64 = 50.0;
/// The first row's top inset, and the step between rows.
const ROW: f64 = 14.0;

/// What one frame's readout draws: each label's text, place, size and colour.
///
/// Two frames whose readouts compare equal, shaped over the same fonts,
/// record the same commands, so the presentation keeps the chunk it
/// published instead of recording a new one, and the overlay damages nothing
/// while its numbers hold still.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Readout {
    labels: Vec<Label>,
}

/// One label of a [`Readout`], at its offset from the top-left corner of the
/// surface in logical pixels.
#[derive(Debug, Clone, PartialEq)]
struct Label {
    text: String,
    at: Offset<f64>,
    font_size: f32,
    color: Color,
}

impl Readout {
    /// The readout of `sample` inside [`BOUNDS`].
    ///
    /// The rows sit at fixed offsets from the top-left corner of the bounds:
    /// a "GPU" label and the fps, a "Frame" label and the frame time, then
    /// the diagnostic line when the sample has one. The fps is coloured green
    /// at 55 and above, yellow from 30, red below.
    pub(crate) fn of(sample: &PerformanceSample<'_>) -> Self {
        let mut labels = Vec::with_capacity(7);
        let mut label = |text: String, x: f64, y: f64, font_size: f32, color: Color| {
            labels.push(Label {
                text,
                at: Offset::new(x, y),
                font_size,
                color,
            });
        };

        let fps = sample.fps;
        let x = BOUNDS.left() + LABEL_X;
        let x_val = BOUNDS.left() + VALUE_X;
        let mut y = BOUNDS.top() + ROW;
        let gray = Color::rgba(130, 130, 130, 255);

        label("GPU".to_owned(), x, y, 11.0, Color::rgba(0, 200, 200, 255));
        let fps_color = if fps >= 55.0 {
            Color::rgba(170, 255, 170, 255)
        } else if fps >= 30.0 {
            Color::rgba(255, 255, 130, 255)
        } else {
            Color::rgba(255, 130, 130, 255)
        };
        label(format!("{fps:.0}"), x_val, y, 11.0, fps_color);
        let fps_w = if fps >= 100.0 {
            24.0
        } else if fps >= 10.0 {
            16.0
        } else {
            8.0
        };
        label("FPS".to_owned(), x_val + fps_w, y, 8.0, gray);
        y += ROW;

        label(
            "Frame".to_owned(),
            x,
            y,
            10.0,
            Color::rgba(200, 100, 255, 255),
        );
        label(
            format!("{:.1}", sample.frame_time_ms),
            x_val,
            y,
            10.0,
            Color::rgba(220, 220, 220, 255),
        );
        label("ms".to_owned(), x_val + 22.0, y, 8.0, gray);

        if let Some(line) = sample.diagnostic_line {
            y += ROW;
            // The densest row: brighter and larger than the unit suffixes so
            // it stays legible after glyph antialiasing and display scaling.
            label(line.to_owned(), x, y, 9.0, Color::rgba(205, 205, 210, 255));
        }
        Self { labels }
    }

    /// The readout recorded over its background, each label shaped through
    /// `text`.
    ///
    /// # Errors
    ///
    /// The [`TextLayoutError`] of the first label that does not shape.
    pub(crate) fn record(&self, text: &mut TextContext) -> Result<DisplayList, TextLayoutError> {
        let mut canvas = Canvas::new();
        canvas.draw_rrect(
            RRect::from_rect_and_radius(BOUNDS, Radius::circular(4.0)),
            &Paint::fill(BACKGROUND),
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
}

/// Draws `label` on `canvas` at `at`, its top-left corner in logical pixels, shaped through
/// `text` as one unwrapped left-to-right line: a label of a debug overlay drawn over the scene
/// (this readout, the build banner of `build_overlay`).
///
/// # Errors
///
/// The [`TextLayoutError`] of a label that does not shape.
pub(crate) fn draw_label(
    canvas: &mut Canvas,
    text: &mut TextContext,
    label: &str,
    at: Offset<f64>,
    font_size: f32,
    color: Color,
) -> Result<(), TextLayoutError> {
    let spans = [(label.to_owned(), None)];
    let paragraph = text
        .shape(&ParagraphSpec {
            font_weight_adjustment: 0,
            spans: &spans,
            default_style: None,
            font_size,
            max_width: None,
            min_width: 0.0,
            text_align: flui_painting::typography::TextAlign::Start,
            line_height: None,
            direction: TextDirection::Ltr,
            max_lines: None,
            ellipsis: None,
        })?
        .to_shaped(None);
    canvas.draw_paragraph(&Arc::new(paragraph), at, color);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use flui_foundation::TextDirection;
    use flui_foundation::geometry::Offset;
    use flui_painting::parley_text::ParagraphSpec;
    use flui_painting::styling::Color;
    use flui_painting::{DisplayList, DrawOp, FontCollection, TextContext};

    use super::{BOUNDS, PerformanceSample, Readout};

    fn readout(text: &mut TextContext, fps: f64, line: Option<&str>) -> DisplayList {
        Readout::of(&PerformanceSample {
            fps,
            frame_time_ms: 16.7,
            diagnostic_line: line,
        })
        .record(text)
        .expect("the overlay fixture has valid label sizes")
    }

    /// Each recorded paragraph as `(text, offset, colour)`, after checking
    /// the first command is the background.
    fn rows(readout: &DisplayList) -> Vec<(String, Offset<f64>, Color)> {
        let mut commands = readout.iter();
        let first = commands.next().expect("the readout records a background");
        assert!(
            matches!(&first.op, DrawOp::RRect { rrect, .. } if rrect.bounding_rect() == BOUNDS),
            "the first command is the background over the bounds: {:?}",
            first.op
        );
        commands
            .map(|command| match &command.op {
                DrawOp::Paragraph {
                    paragraph,
                    offset,
                    color,
                } => (paragraph.text().to_owned(), *offset, *color),
                other => {
                    panic!("the readout records only paragraphs after the background: {other:?}")
                }
            })
            .collect()
    }

    fn expected(
        fps: &str,
        fps_color: Color,
        fps_unit_x: f64,
        line: Option<&str>,
    ) -> Vec<(String, Offset<f64>, Color)> {
        let gray = Color::rgba(130, 130, 130, 255);
        let mut rows = vec![
            (
                "GPU".to_owned(),
                Offset::new(16.0, 22.0),
                Color::rgba(0, 200, 200, 255),
            ),
            (fps.to_owned(), Offset::new(58.0, 22.0), fps_color),
            ("FPS".to_owned(), Offset::new(fps_unit_x, 22.0), gray),
            (
                "Frame".to_owned(),
                Offset::new(16.0, 36.0),
                Color::rgba(200, 100, 255, 255),
            ),
            (
                "16.7".to_owned(),
                Offset::new(58.0, 36.0),
                Color::rgba(220, 220, 220, 255),
            ),
            ("ms".to_owned(), Offset::new(80.0, 36.0), gray),
        ];
        if let Some(line) = line {
            rows.push((
                line.to_owned(),
                Offset::new(16.0, 50.0),
                Color::rgba(205, 205, 210, 255),
            ));
        }
        rows
    }

    fn fast_with_a_diagnostic_line() {
        let mut text = TextContext::new(&FontCollection::new());
        let readout = readout(&mut text, 99.0, Some("present_p99=16ms"));
        assert_eq!(
            rows(&readout),
            expected(
                "99",
                Color::rgba(170, 255, 170, 255),
                74.0,
                Some("present_p99=16ms")
            )
        );
    }

    fn middling_without_a_diagnostic_line() {
        let mut text = TextContext::new(&FontCollection::new());
        let readout = readout(&mut text, 42.0, None);
        assert_eq!(
            rows(&readout),
            expected("42", Color::rgba(255, 255, 130, 255), 74.0, None)
        );
    }

    fn slow_with_a_one_digit_fps() {
        let mut text = TextContext::new(&FontCollection::new());
        let readout = readout(&mut text, 5.0, Some("dropped=3"));
        assert_eq!(
            rows(&readout),
            expected(
                "5",
                Color::rgba(255, 130, 130, 255),
                66.0,
                Some("dropped=3")
            )
        );
    }

    /// Every label is shaped over the collection of the context it is handed:
    /// its runs name that collection's faces and none of a second
    /// collection's. Fails if the readout is shaped through a context of its
    /// own, or its labels are left unshaped (no runs).
    fn readout_is_shaped_over_the_given_collection() {
        fn blob_ids(runs: impl Iterator<Item = u64>) -> BTreeSet<u64> {
            runs.collect()
        }
        let shape = |text: &mut TextContext| {
            let spans = [("GPU 0123456789.".to_owned(), None)];
            text.shape(&ParagraphSpec {
                font_weight_adjustment: 0,
                spans: &spans,
                default_style: None,
                font_size: 11.0,
                max_width: None,
                min_width: 0.0,
                text_align: flui_painting::typography::TextAlign::Start,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .expect("the reference paragraph fixture has valid layout inputs")
            .to_shaped(None)
        };
        let mut given = TextContext::new(&FontCollection::new());
        let mut other = TextContext::new(&FontCollection::new());
        let given_ids = blob_ids(shape(&mut given).runs().map(|run| run.face().blob().id()));
        let other_ids = blob_ids(shape(&mut other).runs().map(|run| run.face().blob().id()));
        assert!(
            given_ids.is_disjoint(&other_ids),
            "two collections share no blob"
        );

        let readout = readout(&mut given, 60.0, Some("dropped=0"));
        let mut labels = 0;
        for command in &readout {
            if let DrawOp::Paragraph { paragraph, .. } = &command.op {
                labels += 1;
                assert!(
                    paragraph.runs().len() > 0,
                    "{:?} is shaped",
                    paragraph.text()
                );
                for run in paragraph.runs() {
                    let id = run.face().blob().id();
                    assert!(
                        given_ids.contains(&id),
                        "{:?} names blob {id}",
                        paragraph.text()
                    );
                    assert!(!other_ids.contains(&id));
                }
            }
        }
        assert_eq!(labels, 7);
    }

    /// The readout records the background and one paragraph per label, at
    /// the label's offset and colour, for fast, middling and slow frames with
    /// and without the diagnostic line; and it is shaped over the caller's
    /// collection.
    #[test]
    fn performance_overlay_readout_rows() {
        crate::table_test::run_table(
            "performance_overlay_readout_rows",
            &[
                ("fast_with_a_diagnostic_line", fast_with_a_diagnostic_line),
                (
                    "middling_without_a_diagnostic_line",
                    middling_without_a_diagnostic_line,
                ),
                ("slow_with_a_one_digit_fps", slow_with_a_one_digit_fps),
                (
                    "readout_is_shaped_over_the_given_collection",
                    readout_is_shaped_over_the_given_collection,
                ),
            ],
        );
    }
}
