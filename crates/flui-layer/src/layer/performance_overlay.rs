//! `PerformanceOverlayLayer` — the frame-statistics readout drawn over the
//! scene.
//!
//! The layer carries its readout already recorded: a [`DisplayList`] whose
//! labels are shaped paragraphs, so the backend clips to the bounds and
//! replays it the way it replays a picture, and never shapes text (ADR-0092).
//! It does not measure anything either: the presentation that owns the frame
//! clock samples fps and frame time, and [`PerformanceOverlayLayer::record`]
//! composes the numbers it is handed through the caller's [`TextContext`], so
//! no clock and no history ring lives in the compositor vocabulary.

use std::sync::Arc;

use flui_foundation::geometry::{Offset, RRect, Radius, Rect};
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_painting::{Canvas, DisplayList, Paint, TextContext};

bitflags::bitflags! {
    /// Which readouts the overlay shows.
    ///
    /// The layer carries the set as-is; today every readout is recorded
    /// regardless (`flui-app`'s config documents the option set as having no
    /// observable effect yet).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PerformanceOverlayOption: u32 {
        /// Frame time and FPS for the raster thread.
        const DISPLAY_RASTER_STATISTICS = 1 << 0;
        /// A histogram of raster-thread frame times.
        const VISUALIZE_RASTER_STATISTICS = 1 << 1;
        /// Frame time and FPS for the UI thread.
        const DISPLAY_ENGINE_STATISTICS = 1 << 2;
        /// A histogram of UI-thread frame times.
        const VISUALIZE_ENGINE_STATISTICS = 1 << 3;
    }
}

/// The numbers one frame's readout shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceSample<'a> {
    /// Frames per second.
    pub fps: f64,
    /// Average frame time in milliseconds.
    pub frame_time_ms: f64,
    /// One extra line of runtime diagnostics (present/input percentiles,
    /// dropped-frame counts), drawn as a third row when present.
    pub diagnostic_line: Option<&'a str>,
}

/// Frame statistics drawn at a fixed rectangle over the scene.
#[derive(Debug, Clone)]
pub struct PerformanceOverlayLayer {
    bounds: Rect<f64>,
    options: PerformanceOverlayOption,
    /// The recorded readout, in the layer's coordinates. `Arc` for the same
    /// reason as `PictureLayer`'s picture: a layer clone shares it.
    readout: Arc<DisplayList>,
}

/// The overlay's background: a translucent near-black.
const BACKGROUND: Color = Color::rgba(10, 10, 15, 200);
/// The label column's inset from the left edge.
const LABEL_X: f64 = 8.0;
/// The value column's inset from the left edge.
const VALUE_X: f64 = 50.0;
/// The first row's top inset, and the step between rows.
const ROW: f64 = 14.0;

impl PerformanceOverlayLayer {
    /// The top-left placement the presentation uses by default.
    #[must_use]
    pub fn default_bounds() -> Rect<f64> {
        Rect::from_ltwh(8.0, 8.0, 480.0, 58.0)
    }

    /// An overlay showing `readout` inside `bounds`. The backend clips the
    /// readout to `bounds`, so ink recorded past them is never drawn.
    #[must_use]
    pub fn new(bounds: Rect<f64>, options: PerformanceOverlayOption, readout: DisplayList) -> Self {
        Self {
            bounds,
            options,
            readout: Arc::new(readout),
        }
    }

    /// An overlay whose readout shows `sample`, shaped through `text`.
    ///
    /// The rows sit at fixed offsets from the top-left corner of `bounds`: a
    /// "GPU" label and the fps, a "Frame" label and the frame time, then the
    /// diagnostic line when the sample has one. The fps is coloured green at
    /// 55 and above, yellow from 30, red below.
    ///
    /// `options` is carried on the layer and not honoured yet: every row is
    /// recorded whatever it names.
    #[must_use]
    pub fn record(
        text: &mut TextContext,
        bounds: Rect<f64>,
        options: PerformanceOverlayOption,
        sample: &PerformanceSample<'_>,
    ) -> Self {
        let mut canvas = Canvas::new();
        canvas.draw_rrect(
            RRect::from_rect_and_radius(bounds, Radius::circular(4.0)),
            &Paint::fill(BACKGROUND),
        );

        let mut label = |text_str: &str, x: f64, y: f64, font_size: f32, color: Color| {
            let spans = [(text_str.to_owned(), None)];
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
                })
                .to_shaped(None);
            canvas.draw_paragraph(&Arc::new(paragraph), Offset::new(x, y), color);
        };

        let fps = sample.fps;
        let x = bounds.left() + LABEL_X;
        let x_val = bounds.left() + VALUE_X;
        let mut y = bounds.top() + ROW;
        let gray = Color::rgba(130, 130, 130, 255);

        label("GPU", x, y, 11.0, Color::rgba(0, 200, 200, 255));
        let fps_color = if fps >= 55.0 {
            Color::rgba(170, 255, 170, 255)
        } else if fps >= 30.0 {
            Color::rgba(255, 255, 130, 255)
        } else {
            Color::rgba(255, 130, 130, 255)
        };
        label(&format!("{fps:.0}"), x_val, y, 11.0, fps_color);
        let fps_w = if fps >= 100.0 {
            24.0
        } else if fps >= 10.0 {
            16.0
        } else {
            8.0
        };
        label("FPS", x_val + fps_w, y, 8.0, gray);
        y += ROW;

        label("Frame", x, y, 10.0, Color::rgba(200, 100, 255, 255));
        label(
            &format!("{:.1}", sample.frame_time_ms),
            x_val,
            y,
            10.0,
            Color::rgba(220, 220, 220, 255),
        );
        label("ms", x_val + 22.0, y, 8.0, gray);

        if let Some(line) = sample.diagnostic_line {
            y += ROW;
            // The densest row: brighter and larger than the unit suffixes so
            // it stays legible after glyph antialiasing and display scaling.
            label(line, x, y, 9.0, Color::rgba(205, 205, 210, 255));
        }

        Self::new(bounds, options, canvas.finish())
    }

    /// The recorded readout, in the layer's coordinates.
    #[inline]
    pub fn readout(&self) -> &DisplayList {
        &self.readout
    }

    /// Which readouts to show.
    #[inline]
    pub fn options(&self) -> PerformanceOverlayOption {
        self.options
    }

    /// Where the overlay is drawn; the backend clips the readout to it.
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
        self.bounds
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use flui_foundation::geometry::{Offset, Rect};
    use flui_painting::parley_text::ParagraphSpec;
    use flui_painting::styling::Color;
    use flui_painting::typography::TextDirection;
    use flui_painting::{DrawOp, FontCollection, TextContext};

    use super::{PerformanceOverlayLayer, PerformanceOverlayOption, PerformanceSample};

    const BOUNDS: Rect<f64> = Rect::from_ltrb(8.0, 8.0, 488.0, 66.0);

    fn record(text: &mut TextContext, fps: f64, line: Option<&str>) -> PerformanceOverlayLayer {
        PerformanceOverlayLayer::record(
            text,
            BOUNDS,
            PerformanceOverlayOption::all(),
            &PerformanceSample {
                fps,
                frame_time_ms: 16.7,
                diagnostic_line: line,
            },
        )
    }

    /// Each recorded paragraph as `(text, offset, colour)`, after checking
    /// the first command is the background.
    fn rows(layer: &PerformanceOverlayLayer) -> Vec<(String, Offset<f64>, Color)> {
        let mut commands = layer.readout().iter();
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
        let layer = record(&mut text, 99.0, Some("present_p99=16ms"));
        assert_eq!(
            rows(&layer),
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
        let layer = record(&mut text, 42.0, None);
        assert_eq!(
            rows(&layer),
            expected("42", Color::rgba(255, 255, 130, 255), 74.0, None)
        );
    }

    fn slow_with_a_one_digit_fps() {
        let mut text = TextContext::new(&FontCollection::new());
        let layer = record(&mut text, 5.0, Some("dropped=3"));
        assert_eq!(
            rows(&layer),
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

        let layer = record(&mut given, 60.0, Some("dropped=0"));
        let mut labels = 0;
        for command in layer.readout() {
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
        let rows: [(&str, fn()); 4] = [
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
        ];
        let failed: Vec<&str> = rows
            .iter()
            .filter(|(_, row)| std::panic::catch_unwind(*row).is_err())
            .map(|(name, _)| *name)
            .collect();
        assert!(failed.is_empty(), "failing rows: {failed:?}");
    }
}
