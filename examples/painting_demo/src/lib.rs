//! FLUI Painting Demo: the `Canvas` drawing primitives in a browser, drawn by
//! an ordinary FLUI application.
//!
//! `run_app` starts the web runner, which takes the page's
//! `<canvas id="flui-canvas">` as the application's window. The root view is
//! a scrolling column of sections. Each is a `Text` title above a
//! `CustomPaint` whose `CustomPainter` draws that section's geometry at the
//! size it is given, with the captions as `Text` widgets under or beside it.
//! A painter draws geometry only: every word on the page is a `Text` widget.
//!
//! Build with:
//!   cd examples/painting_demo && wasm-pack build --target web --out-dir pkg
//! Then serve the directory with any HTTP server and open index.html.

#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use std::any::Any;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use flui::geometry::{Offset, Point, RRect, Rect, Size};
use flui::painting::Canvas;
use flui::painting::paint::{Paint, Path, Shader};
use flui::painting::styling::Color;
use flui::painting::typography::TextStyle;
use flui::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

/// Entry point invoked by the generated JS glue on module load: runs the demo
/// in the page's canvas.
///
/// # Errors
/// The application could not start; the error's text names why.
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    run_app(PaintingDemo).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// The page's background (`index.html`'s `#0d0d1a`).
const BACKGROUND: Color = Color::rgba(13, 13, 26, 255);
/// The space a figure keeps from the edges of its column, in logical pixels.
const INSET: f64 = 10.0;
/// The height of one row of the line sections: a line and its caption.
const LINE_ROW: f64 = 18.0;

/// The root: every section, top to bottom, scrolling within the window.
#[derive(Clone, StatelessView)]
struct PaintingDemo;

impl StatelessView for PaintingDemo {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let sections: Vec<BoxedView> = vec![
            section(
                "1. Basic Shapes",
                figure_over(
                    Drawing::BasicShapes,
                    90.0,
                    &["Red", "Green", "Blue", "Yellow", "Cyan", "Magenta"],
                ),
            ),
            section(
                "2. Rounded Rectangles",
                figure_over(
                    Drawing::RoundedRects,
                    80.0,
                    &["r=5", "r=15", "r=30", "r=50"],
                ),
            ),
            section(
                "3. Circles & Ovals",
                figure(Drawing::CirclesAndOvals, 110.0).boxed(),
            ),
            section(
                "4. Lines & Strokes",
                figure_over(
                    Drawing::LinesAndStrokes,
                    5.0 * LINE_ROW,
                    &[
                        "1, 2, 3, 5, 8 px",
                        "stroke rect",
                        "stroke rrect",
                        "stroke circle",
                    ],
                ),
            ),
            section(
                "5. Dashed Lines",
                figure_beside(
                    Drawing::DashedLines,
                    &["10-5", "20-10", "5-5-15-5", "2-8 (dots)"],
                ),
            ),
            section(
                "6. Paths & Polygons",
                figure_over(
                    Drawing::Paths,
                    120.0,
                    &[
                        "Triangle", "Star", "Pentagon", "Hexagon", "Octagon", "Zigzag",
                    ],
                ),
            ),
            section(
                "7. Arcs",
                figure_over(
                    Drawing::Arcs,
                    100.0,
                    &["90 deg", "180 deg", "45-225 deg", "270 deg", "Open arcs"],
                ),
            ),
            section(
                "8. Gradients (Linear, Radial, Sweep)",
                figure_over(
                    Drawing::Gradients,
                    100.0,
                    &[
                        "Linear (rainbow)",
                        "Linear (diagonal)",
                        "Radial",
                        "Sweep",
                        "Gradient rrect",
                    ],
                ),
            ),
            section(
                "9. Transforms (translate, rotate, scale)",
                figure_over(
                    Drawing::Transforms,
                    140.0,
                    &["Translate", "Rotate", "Scale", "Combined", "Nested"],
                ),
            ),
            section(
                "10. Clipping",
                figure_over(
                    Drawing::Clipping,
                    100.0,
                    &["clip_rect", "Clipped circles", "Nested clips"],
                ),
            ),
            section(
                "11. Double Rounded Rect (Frame)",
                figure_over(
                    Drawing::DoubleRoundedRects,
                    100.0,
                    &["Thick frame", "Gradient frame", "Thin frame", "Asymmetric"],
                ),
            ),
            section("12. Text Rendering", text_sizes()),
            section(
                "13. Opacity & Blending",
                figure_over(
                    Drawing::Opacity,
                    100.0,
                    &["Decreasing alpha", "RGB overlap", "Alpha gradient"],
                ),
            ),
        ];
        ColoredBox::new(BACKGROUND).child(
            SingleChildScrollView::new().child(
                Padding::all(30.0).child(
                    Column::new(sections)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .spacing(30.0),
                ),
            ),
        )
    }
}

/// A section: its title above `body`, both as wide as the column.
fn section(title: &str, body: BoxedView) -> BoxedView {
    Column::new((
        Text::new(title).style(
            TextStyle::new()
                .with_color(Color::rgba(0, 210, 255, 255))
                .with_font_size(22.0),
        ),
        SizedBox::height(12.0),
        body,
    ))
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .boxed()
}

/// `drawing`, `height` tall and as wide as the column gives it (the column
/// stretches its children, so the width is tight and the painter gets it all).
fn figure(drawing: Drawing, height: f64) -> CustomPaint {
    CustomPaint::new()
        .painter(DrawingPainter(drawing))
        .size(Size::new(0.0, height))
}

/// `drawing` over one caption per column of it, each centred under its figure.
fn figure_over(drawing: Drawing, height: f64, captions: &[&str]) -> BoxedView {
    let cells: Vec<BoxedView> = captions
        .iter()
        .map(|caption| Expanded::new(Center::new().child(self::caption(caption))).boxed())
        .collect();
    Column::new((
        figure(drawing, height),
        SizedBox::height(6.0),
        Row::new(cells),
    ))
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .boxed()
}

/// `drawing`, one row of [`LINE_ROW`] per caption, beside the captions, each
/// level with its row.
fn figure_beside(drawing: Drawing, captions: &[&str]) -> BoxedView {
    let rows: Vec<BoxedView> = captions
        .iter()
        .map(|caption| {
            SizedBox::height(LINE_ROW)
                .child(Align::new(Alignment::CENTER_LEFT).child(self::caption(caption)))
                .boxed()
        })
        .collect();
    let height = LINE_ROW * captions.len() as f64;
    Row::new((
        Expanded::new(figure(drawing, height)),
        SizedBox::width(120.0).child(Column::new(rows)),
    ))
    .boxed()
}

fn caption(text: &str) -> Text {
    Text::new(text).style(
        TextStyle::new()
            .with_color(Color::LIGHT_GRAY)
            .with_font_size(12.0),
    )
}

/// 12. Text at five sizes, one line each.
fn text_sizes() -> BoxedView {
    let lines: Vec<BoxedView> = [12.0, 16.0, 20.0, 28.0, 36.0]
        .into_iter()
        .map(|size: f64| {
            Text::new(format!("Font size {size}px — Hello FLUI!"))
                .style(
                    TextStyle::new()
                        .with_color(Color::WHITE)
                        .with_font_size(size),
                )
                .boxed()
        })
        .collect();
    Column::new(lines)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(8.0)
        .boxed()
}

/// The geometry one section draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawing {
    BasicShapes,
    RoundedRects,
    CirclesAndOvals,
    LinesAndStrokes,
    DashedLines,
    Paths,
    Arcs,
    Gradients,
    Transforms,
    Clipping,
    DoubleRoundedRects,
    Opacity,
}

/// Paints one [`Drawing`] at the size its `CustomPaint` is given.
#[derive(Debug)]
struct DrawingPainter(Drawing);

impl CustomPainter for DrawingPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        match self.0 {
            Drawing::BasicShapes => draw_basic_shapes(canvas, size),
            Drawing::RoundedRects => draw_rounded_rects(canvas, size),
            Drawing::CirclesAndOvals => draw_circles_and_ovals(canvas, size),
            Drawing::LinesAndStrokes => draw_lines_and_strokes(canvas, size),
            Drawing::DashedLines => draw_dashed_lines(canvas, size),
            Drawing::Paths => draw_paths(canvas, size),
            Drawing::Arcs => draw_arcs(canvas, size),
            Drawing::Gradients => draw_gradients(canvas, size),
            Drawing::Transforms => draw_transforms(canvas, size),
            Drawing::Clipping => draw_clipping(canvas, size),
            Drawing::DoubleRoundedRects => draw_drrect(canvas, size),
            Drawing::Opacity => draw_opacity(canvas, size),
        }
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        (old_delegate as &dyn Any)
            .downcast_ref::<Self>()
            .is_none_or(|old| old.0 != self.0)
    }
}

// ============================================================
// Geometry helpers
// ============================================================

fn pt(x: f64, y: f64) -> Point<f64> {
    Point::new(x, y)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect<f64> {
    Rect::from_xywh(x, y, w, h)
}

fn ofs(dx: f64, dy: f64) -> Offset<f64> {
    Offset::new(dx, dy)
}

/// The `count` equal columns `size` splits into, left to right; the captions
/// under a figure are laid out in the same columns.
fn columns(size: Size, count: usize) -> impl Iterator<Item = Rect<f64>> {
    let width = size.width / count as f64;
    (0..count).map(move |i| rect(i as f64 * width, 0.0, width, size.height))
}

/// A `width` × `height` box centred in `cell`, narrowed to keep [`INSET`]
/// from its sides.
fn centered(cell: Rect<f64>, width: f64, height: f64) -> Rect<f64> {
    let width = width.min(cell.width() - 2.0 * INSET).max(0.0);
    let height = height.min(cell.height()).max(0.0);
    let center = cell.center();
    rect(
        center.x - width / 2.0,
        center.y - height / 2.0,
        width,
        height,
    )
}

/// The scale that fits a figure `extent` across into `cell`, never above 1.
fn fit(cell: Rect<f64>, extent: f64) -> f64 {
    ((cell.width() - 2.0 * INSET) / extent)
        .min(cell.height() / extent)
        .clamp(0.0, 1.0)
}

fn make_star(center: Point<f64>, outer_r: f64, inner_r: f64, points: usize) -> Path {
    let mut pts = Vec::with_capacity(points * 2);
    for i in 0..(points * 2) {
        let angle = (i as f64) * PI / points as f64 - FRAC_PI_2;
        let r = if i % 2 == 0 { outer_r } else { inner_r };
        pts.push(pt(center.x + angle.cos() * r, center.y + angle.sin() * r));
    }
    Path::polygon(&pts)
}

fn make_regular_polygon(center: Point<f64>, radius: f64, sides: usize) -> Path {
    let mut pts = Vec::with_capacity(sides);
    for i in 0..sides {
        let angle = (i as f64) * TAU / sides as f64 - FRAC_PI_2;
        pts.push(pt(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        ));
    }
    Path::polygon(&pts)
}

// ============================================================
// Demo sections, each drawn in its own coordinates over `size`
// ============================================================

/// 1. Basic filled rectangles
fn draw_basic_shapes(canvas: &mut Canvas, size: Size) {
    let colors = [
        Color::RED,
        Color::GREEN,
        Color::BLUE,
        Color::YELLOW,
        Color::CYAN,
        Color::MAGENTA,
    ];
    for (cell, color) in columns(size, colors.len()).zip(colors) {
        canvas.draw_rect(centered(cell, 170.0, 90.0), &Paint::fill(color));
    }
}

/// 2. Rounded rects with varying radii
fn draw_rounded_rects(canvas: &mut Canvas, size: Size) {
    let radii = [5.0, 15.0, 30.0, 50.0];
    let colors = [
        Color::rgba(128, 0, 255, 255),
        Color::rgba(255, 100, 50, 255),
        Color::rgba(50, 200, 100, 255),
        Color::rgba(200, 50, 200, 255),
    ];
    for (cell, (radius, color)) in columns(size, radii.len()).zip(radii.into_iter().zip(colors)) {
        let rrect = RRect::from_rect_circular(centered(cell, 250.0, 80.0), radius);
        canvas.draw_rrect(rrect, &Paint::fill(color));
    }
}

/// 3. Circles and ovals
fn draw_circles_and_ovals(canvas: &mut Canvas, size: Size) {
    let circle_colors = [
        Color::rgba(255, 80, 80, 255),
        Color::rgba(80, 255, 80, 255),
        Color::rgba(80, 80, 255, 255),
        Color::rgba(255, 200, 50, 255),
    ];
    let oval_colors = [
        Color::rgba(255, 128, 0, 200),
        Color::rgba(0, 200, 200, 200),
        Color::rgba(200, 100, 255, 200),
    ];
    let mut cells = columns(size, circle_colors.len() + oval_colors.len());

    // Filled circles, each stroked over in its own colour.
    for (cell, color) in cells.by_ref().zip(circle_colors) {
        let radius = 50.0 * fit(cell, 100.0);
        canvas.draw_circle(cell.center(), radius, &Paint::fill(color));
        canvas.draw_circle(cell.center(), radius, &Paint::stroke(color, 2.0));
    }

    for (cell, color) in cells.zip(oval_colors) {
        canvas.draw_oval(centered(cell, 170.0, 100.0), &Paint::fill(color));
    }
}

/// 4. Lines of five widths, and three stroked shapes
fn draw_lines_and_strokes(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 4);
    let widths = [1.0, 2.0, 3.0, 5.0, 8.0];
    let line_colors = [
        Color::rgba(255, 50, 50, 255),
        Color::rgba(50, 255, 50, 255),
        Color::rgba(50, 50, 255, 255),
        Color::rgba(255, 255, 50, 255),
        Color::rgba(255, 50, 255, 255),
    ];
    if let Some(cell) = cells.next() {
        for (i, (width, color)) in widths.into_iter().zip(line_colors).enumerate() {
            let y = (i as f64 + 0.5) * LINE_ROW;
            canvas.draw_line(
                pt(cell.left() + INSET, y),
                pt(cell.right() - INSET, y),
                &Paint::stroke(color, width),
            );
        }
    }
    if let Some(cell) = cells.next() {
        canvas.draw_rect(
            centered(cell, 150.0, 80.0),
            &Paint::stroke(Color::WHITE, 2.0),
        );
    }
    if let Some(cell) = cells.next() {
        let rrect = RRect::from_rect_circular(centered(cell, 150.0, 80.0), 15.0);
        canvas.draw_rrect(rrect, &Paint::stroke(Color::rgba(100, 200, 255, 255), 3.0));
    }
    if let Some(cell) = cells.next() {
        canvas.draw_circle(
            cell.center(),
            40.0 * fit(cell, 80.0),
            &Paint::stroke(Color::rgba(255, 200, 100, 255), 3.0),
        );
    }
}

/// 5. Dashed lines, one per row of [`LINE_ROW`]
fn draw_dashed_lines(canvas: &mut Canvas, size: Size) {
    let patterns: [&[f64]; 4] = [
        &[10.0, 5.0],
        &[20.0, 10.0],
        &[5.0, 5.0, 15.0, 5.0],
        &[2.0, 8.0],
    ];
    for (i, intervals) in patterns.into_iter().enumerate() {
        let y = (i as f64 + 0.5) * LINE_ROW;
        let dash_paint =
            Paint::stroke(Color::rgba(200, 200, 255, 255), 2.0).with_dash(intervals.to_vec(), 0.0);
        canvas.draw_line(pt(INSET, y), pt(size.width - INSET, y), &dash_paint);
    }
}

/// 6. Paths & polygons (triangle, star, pentagon, hexagon, octagon, zigzag)
fn draw_paths(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 6);
    let Some(cell) = cells.next() else { return };
    let tri = centered(cell, 120.0, 120.0);
    let triangle = Path::polygon(&[
        pt(tri.center().x, tri.top()),
        pt(tri.left(), tri.bottom()),
        pt(tri.right(), tri.bottom()),
    ]);
    canvas.draw_path(&triangle, &Paint::fill(Color::rgba(255, 100, 50, 255)));

    let Some(cell) = cells.next() else { return };
    let scale = fit(cell, 120.0);
    let star = make_star(cell.center(), 60.0 * scale, 25.0 * scale, 5);
    canvas.draw_path(&star, &Paint::fill(Color::rgba(255, 220, 50, 255)));

    let polygons = [
        (5, Paint::fill(Color::rgba(50, 200, 150, 255))),
        (6, Paint::fill(Color::rgba(150, 50, 255, 255))),
        (8, Paint::stroke(Color::rgba(255, 100, 200, 255), 2.0)),
    ];
    for (cell, (sides, paint)) in cells.by_ref().zip(polygons) {
        let polygon = make_regular_polygon(cell.center(), 55.0 * fit(cell, 110.0), sides);
        canvas.draw_path(&polygon, &paint);
    }

    let Some(cell) = cells.next() else { return };
    let zig = centered(cell, 160.0, 120.0);
    let step = zig.width() / 4.0;
    let mut zigzag = Path::new();
    zigzag.move_to(pt(zig.left(), zig.bottom()));
    zigzag.line_to(pt(zig.left() + step, zig.top()));
    zigzag.line_to(pt(zig.left() + 2.0 * step, zig.center().y));
    zigzag.line_to(pt(zig.left() + 3.0 * step, zig.top()));
    zigzag.line_to(pt(zig.right(), zig.bottom()));
    zigzag.close();
    canvas.draw_path(&zigzag, &Paint::fill(Color::rgba(100, 200, 255, 200)));
}

/// 7. Arcs: four pie slices, then the same four sweeps as open arcs
fn draw_arcs(canvas: &mut Canvas, size: Size) {
    let arcs = [
        (0.0, FRAC_PI_2, Color::rgba(255, 80, 80, 220)),
        (0.0, PI, Color::rgba(80, 255, 80, 220)),
        (FRAC_PI_4, PI, Color::rgba(80, 80, 255, 220)),
        (0.0, TAU * 0.75, Color::rgba(255, 200, 50, 220)),
    ];
    let mut cells = columns(size, arcs.len() + 1);

    // Pie slices (use_center = true)
    for (cell, (start, sweep, color)) in cells.by_ref().zip(arcs) {
        let side = 100.0 * fit(cell, 100.0);
        canvas.draw_arc(
            centered(cell, side, side),
            start,
            sweep,
            true,
            &Paint::fill(color),
        );
    }

    // Open arcs (use_center = false), two by two
    let Some(cell) = cells.next() else { return };
    let side = 50.0 * fit(cell, 100.0);
    let grid = centered(cell, 2.0 * side, 2.0 * side);
    for (i, (start, sweep, color)) in arcs.into_iter().enumerate() {
        let x = grid.left() + (i % 2) as f64 * side;
        let y = grid.top() + (i / 2) as f64 * side;
        canvas.draw_arc(
            rect(x, y, side, side).inset(5.0),
            start,
            sweep,
            false,
            &Paint::stroke(color, 3.0),
        );
    }
}

/// 8. Gradients
fn draw_gradients(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 5);

    // Linear gradient (horizontal rainbow)
    let Some(cell) = cells.next() else { return };
    let bar = centered(cell, 300.0, 100.0);
    let linear = Shader::simple_linear(
        ofs(bar.left(), bar.top()),
        ofs(bar.right(), bar.top()),
        vec![
            Color::RED,
            Color::YELLOW,
            Color::GREEN,
            Color::CYAN,
            Color::BLUE,
        ],
    );
    canvas.draw_rect(bar, &Paint::fill(Color::WHITE).with_shader(linear));

    // Linear gradient (diagonal)
    let Some(cell) = cells.next() else { return };
    let bar = centered(cell, 270.0, 100.0);
    let diagonal = Shader::simple_linear(
        ofs(bar.left(), bar.top()),
        ofs(bar.right(), bar.bottom()),
        vec![Color::rgba(255, 0, 128, 255), Color::rgba(0, 128, 255, 255)],
    );
    canvas.draw_rect(bar, &Paint::fill(Color::WHITE).with_shader(diagonal));

    // Radial gradient
    let Some(cell) = cells.next() else { return };
    let center = cell.center();
    let radius = 50.0 * fit(cell, 100.0);
    let radial = Shader::simple_radial(
        ofs(center.x, center.y),
        radius,
        vec![
            Color::WHITE,
            Color::rgba(255, 100, 50, 255),
            Color::rgba(50, 0, 100, 255),
        ],
    );
    canvas.draw_circle(
        center,
        radius,
        &Paint::fill(Color::WHITE).with_shader(radial),
    );

    // Sweep gradient
    let Some(cell) = cells.next() else { return };
    let center = cell.center();
    let sweep = Shader::simple_sweep(
        ofs(center.x, center.y),
        vec![
            Color::RED,
            Color::YELLOW,
            Color::GREEN,
            Color::CYAN,
            Color::BLUE,
            Color::MAGENTA,
            Color::RED,
        ],
    );
    canvas.draw_circle(
        center,
        50.0 * fit(cell, 100.0),
        &Paint::fill(Color::WHITE).with_shader(sweep),
    );

    // Gradient on rounded rect
    let Some(cell) = cells.next() else { return };
    let bar = centered(cell, 170.0, 100.0);
    let rrect_grad = Shader::simple_linear(
        ofs(bar.left(), bar.top()),
        ofs(bar.right(), bar.bottom()),
        vec![
            Color::rgba(255, 50, 200, 255),
            Color::rgba(50, 200, 255, 255),
        ],
    );
    canvas.draw_rrect(
        RRect::from_rect_circular(bar, 20.0),
        &Paint::fill(Color::WHITE).with_shader(rrect_grad),
    );
}

/// 9. Transforms, each figure about the centre of its column
fn draw_transforms(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 5);
    // Every figure fits in a square this wide before its column scales it.
    let extent = 140.0;
    let mut about = |canvas: &mut Canvas, draw: &dyn Fn(&mut Canvas)| {
        let Some(cell) = cells.next() else { return };
        let center = cell.center();
        let scale = fit(cell, extent);
        canvas.save();
        canvas.translate(center.x, center.y);
        canvas.scale(scale, scale);
        draw(canvas);
        canvas.restore();
    };

    // Translate
    about(canvas, &|canvas| {
        canvas.draw_rect(
            rect(-40.0, -30.0, 80.0, 60.0),
            &Paint::fill(Color::rgba(200, 80, 80, 200)),
        );
    });

    // Rotate (a fan of rotated rects)
    about(canvas, &|canvas| {
        let colors = [
            Color::rgba(255, 50, 50, 100),
            Color::rgba(50, 255, 50, 100),
            Color::rgba(50, 50, 255, 100),
            Color::rgba(255, 255, 50, 100),
            Color::rgba(255, 50, 255, 100),
            Color::rgba(50, 255, 255, 100),
        ];
        for (i, color) in colors.into_iter().enumerate() {
            canvas.save();
            canvas.rotate(i as f64 * PI / 6.0);
            canvas.draw_rect(rect(-50.0, -15.0, 100.0, 30.0), &Paint::fill(color));
            canvas.restore();
        }
    });

    // Scale
    about(canvas, &|canvas| {
        for (i, s) in [0.5_f64, 0.75, 1.0, 1.25].into_iter().enumerate() {
            canvas.save();
            canvas.scale(s, s);
            let alpha = 100 + i as u8 * 40;
            canvas.draw_rect(
                rect(-30.0, -20.0, 60.0, 40.0),
                &Paint::fill(Color::rgba(100, 200, 255, alpha)),
            );
            canvas.restore();
        }
    });

    // Combined: rotate + scale
    about(canvas, &|canvas| {
        canvas.rotate(0.3);
        canvas.scale(1.5, 0.8);
        canvas.draw_rect(
            rect(-40.0, -25.0, 80.0, 50.0),
            &Paint::fill(Color::rgba(255, 150, 50, 200)),
        );
    });

    // Nested transforms
    about(canvas, &|canvas| {
        canvas.draw_rect(
            rect(-50.0, -50.0, 100.0, 100.0),
            &Paint::stroke(Color::GRAY, 1.0),
        );
        canvas.save();
        canvas.rotate(FRAC_PI_4);
        canvas.draw_rect(
            rect(-35.0, -35.0, 70.0, 70.0),
            &Paint::fill(Color::rgba(150, 100, 255, 180)),
        );
        canvas.save();
        canvas.rotate(FRAC_PI_4);
        canvas.draw_rect(
            rect(-20.0, -20.0, 40.0, 40.0),
            &Paint::fill(Color::rgba(255, 100, 150, 180)),
        );
        canvas.restore();
        canvas.restore();
    });
}

/// 10. Clipping
fn draw_clipping(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 3);
    let outline = Paint::stroke(Color::WHITE, 1.0);

    // A gradient larger than its clip rect.
    let Some(cell) = cells.next() else { return };
    let window = centered(cell, 200.0, 100.0);
    let spill = window.inflate(50.0, 50.0);
    canvas.save();
    // `Canvas::clip_rect` clips with a hard edge: the scissor.
    canvas.clip_rect(window);
    let grad = Shader::simple_linear(
        ofs(spill.left(), spill.top()),
        ofs(spill.right(), spill.bottom()),
        vec![Color::RED, Color::BLUE],
    );
    canvas.draw_rect(spill, &Paint::fill(Color::WHITE).with_shader(grad));
    canvas.restore();
    canvas.draw_rect(window, &outline);

    // Circles cut by their clip rect.
    let Some(cell) = cells.next() else { return };
    let window = centered(cell, 200.0, 100.0);
    canvas.save();
    canvas.clip_rect(window);
    for i in 0..8_u8 {
        let cx = window.left() + f64::from(i) * window.width() * 0.15;
        let color = Color::rgba(i * 35, 255 - i * 30, i * 25 + 50, 255);
        canvas.draw_circle(
            pt(cx, window.center().y),
            window.height() * 0.4,
            &Paint::fill(color),
        );
    }
    canvas.restore();
    canvas.draw_rect(window, &outline);

    // Two clips nested in a third.
    let Some(cell) = cells.next() else { return };
    let window = centered(cell, 250.0, 100.0);
    let pane = (window.width() - 50.0) / 2.0;
    canvas.save();
    canvas.clip_rect(window);
    canvas.draw_rect(window, &Paint::fill(Color::rgba(40, 40, 60, 255)));
    for (left, color) in [
        (window.left() + 20.0, Color::rgba(255, 80, 80, 200)),
        (window.left() + 30.0 + pane, Color::rgba(80, 80, 255, 200)),
    ] {
        canvas.save();
        canvas.clip_rect(rect(
            left,
            window.top() + 10.0,
            pane,
            window.height() - 20.0,
        ));
        canvas.draw_rect(window.inflate(20.0, 10.0), &Paint::fill(color));
        canvas.restore();
    }
    canvas.restore();
    canvas.draw_rect(window, &outline);
}

/// 11. Double rounded rects: a frame between an outer and an inner rrect
fn draw_drrect(canvas: &mut Canvas, size: Size) {
    let frame_grad = |frame: Rect<f64>| {
        Shader::simple_linear(
            ofs(frame.left(), frame.top()),
            ofs(frame.right(), frame.bottom()),
            vec![
                Color::rgba(255, 50, 200, 255),
                Color::rgba(50, 200, 255, 255),
            ],
        )
    };
    // (outer radius, border width, inner radius, gradient or colour)
    let frames: [(f64, f64, f64, Option<Color>); 4] = [
        (20.0, 15.0, 10.0, Some(Color::rgba(255, 100, 50, 255))),
        (30.0, 10.0, 20.0, None),
        (15.0, 4.0, 12.0, Some(Color::rgba(100, 255, 150, 255))),
        (40.0, 20.0, 5.0, Some(Color::rgba(255, 200, 50, 255))),
    ];
    for (cell, (outer_radius, border, inner_radius, color)) in columns(size, 4).zip(frames) {
        let frame = centered(cell, 200.0, 100.0);
        let outer = RRect::from_rect_circular(frame, outer_radius);
        let inner = RRect::from_rect_circular(frame.inset(border), inner_radius);
        let paint = match color {
            Some(color) => Paint::fill(color),
            None => Paint::fill(Color::WHITE).with_shader(frame_grad(frame)),
        };
        canvas.draw_drrect(outer, inner, &paint);
    }
}

/// 13. Opacity & blending
fn draw_opacity(canvas: &mut Canvas, size: Size) {
    let mut cells = columns(size, 3);

    // Overlapping rectangles of decreasing alpha.
    let Some(cell) = cells.next() else { return };
    let strip = centered(cell, 300.0, 80.0);
    let step = strip.width() / 5.0;
    for (i, alpha) in [200, 150, 100, 50].into_iter().enumerate() {
        canvas.draw_rect(
            rect(
                strip.left() + i as f64 * step,
                strip.top(),
                2.0 * step,
                strip.height(),
            ),
            &Paint::fill(Color::rgba(255, 50, 50, alpha)),
        );
    }

    // RGB overlap (additive-like with transparency).
    let Some(cell) = cells.next() else { return };
    let center = cell.center();
    let scale = fit(cell, 160.0);
    for (dx, dy, color) in [
        (-35.0, -15.0, Color::rgba(255, 0, 0, 120)),
        (0.0, 15.0, Color::rgba(0, 255, 0, 120)),
        (35.0, -15.0, Color::rgba(0, 0, 255, 120)),
    ] {
        canvas.draw_circle(
            pt(center.x + dx * scale, center.y + dy * scale),
            45.0 * scale,
            &Paint::fill(color),
        );
    }

    // Bars of rising alpha.
    let Some(cell) = cells.next() else { return };
    let strip = centered(cell, 500.0, 80.0);
    let step = strip.width() / 10.0;
    for i in 0..10_u8 {
        let alpha = (f64::from(i + 1) * 25.5) as u8;
        canvas.draw_rect(
            rect(
                strip.left() + f64::from(i) * step,
                strip.top(),
                step * 0.9,
                strip.height(),
            ),
            &Paint::fill(Color::rgba(100, 200, 255, alpha)),
        );
    }
}
