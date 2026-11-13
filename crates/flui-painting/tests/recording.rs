//! Recording contracts of `Canvas` and `DisplayList`: save/restore balance,
//! isolated appends, paint interning, and hand-off to another thread.

use std::sync::Arc;
use std::thread;

use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp, NonFiniteOps, Paint};

pub(crate) fn save_restore_tracks_the_save_count() {
    let mut canvas = Canvas::new();
    assert_eq!(canvas.save_count(), 1);
    canvas.save();
    assert_eq!(canvas.save_count(), 2);
    canvas.translate(50.0, 50.0);
    canvas.save();
    assert_eq!(canvas.save_count(), 3);
    canvas.restore();
    assert_eq!(canvas.save_count(), 2);
    canvas.restore();
    assert_eq!(canvas.save_count(), 1);
}

pub(crate) fn shear_factors_map_the_named_axes_in_recorded_commands() {
    let list = flui_painting::testing::record(|canvas| {
        canvas.translate(10.0, 20.0);
        canvas.skew(2.0, 3.0);
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 1.0, 1.0),
            &Paint::fill(Color::RED),
        );
    });
    let command = list.iter().next().expect("the rectangle was recorded");
    assert_eq!(command.transform.transform_point(4.0, 5.0), (24.0, 37.0));
    assert_eq!(list.bounds(), Some(Rect::from_ltrb(10.0, 20.0, 13.0, 24.0)));
}

/// `Canvas::finish` wires a `debug_assert!` to catch unrestored `save()`
/// calls; release builds finalise silently, so the row is debug-only.
#[cfg(debug_assertions)]
pub(crate) fn finish_panics_in_debug_on_unrestored_save() {
    let payload = std::panic::catch_unwind(|| {
        let mut canvas = Canvas::new();
        canvas.save();
        canvas.translate(50.0, 50.0);
        let _ = canvas.finish();
    })
    .expect_err("finish() must reject an unrestored save() in debug builds");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("unrestored save() calls"),
        "unexpected panic message: {message:?}"
    );
}

pub(crate) fn isolated_append_preserves_bounds_and_scopes_the_run() {
    let rect = Rect::from_xywh(100.0, 200.0, 30.0, 40.0);
    let run = flui_painting::testing::record(|canvas| {
        canvas.clip_rect(rect);
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
    });
    let mut combined = DisplayList::new();
    combined.append_isolated(DisplayList::new());
    assert!(combined.is_empty());
    combined.append_isolated(run);
    assert_eq!(combined.bounds(), Some(rect));
    let ops: Vec<&DrawOp> = combined.iter().map(|c| &c.op).collect();
    assert!(matches!(
        ops.as_slice(),
        [
            DrawOp::Save,
            DrawOp::ClipRect { .. },
            DrawOp::Rect { .. },
            DrawOp::Restore,
        ]
    ));
    combined.append_isolated(DisplayList::new());
    assert_eq!(combined.len(), 4);
    assert_eq!(combined.bounds(), Some(rect));
}

/// A run that only draws changes no replay state, so it is spliced as it
/// stands; a run that clips keeps the scope that ends its clip before the
/// next run.
pub(crate) fn isolated_append_scopes_only_a_run_that_changes_replay_state() {
    let first = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
    let clip = Rect::from_xywh(20.0, 0.0, 10.0, 10.0);
    let last = Rect::from_xywh(40.0, 0.0, 10.0, 10.0);
    let draws = |rect| {
        flui_painting::testing::record(move |canvas| {
            canvas.draw_rect(rect, &Paint::fill(Color::RED));
        })
    };
    let clips = flui_painting::testing::record(|canvas| {
        canvas.clip_rect(clip);
        canvas.draw_rect(clip, &Paint::fill(Color::BLUE));
    });
    let mut combined = DisplayList::new();
    combined.append_isolated(draws(first));
    combined.append_isolated(clips);
    combined.append_isolated(draws(last));
    let ops: Vec<&DrawOp> = combined.iter().map(|c| &c.op).collect();
    assert!(
        matches!(
            ops.as_slice(),
            [
                DrawOp::Rect { .. },
                DrawOp::Save,
                DrawOp::ClipRect { .. },
                DrawOp::Rect { .. },
                DrawOp::Restore,
                DrawOp::Rect { .. },
            ]
        ),
        "{ops:#?}"
    );
    assert_eq!(combined.bounds(), Some(first.union(&last)));
}

fn rect_paint(cmd: &DrawCommand) -> &Arc<Paint> {
    match &cmd.op {
        DrawOp::Rect { paint, .. } => paint,
        other => panic!("expected DrawRect, got {other:?}"),
    }
}

/// Two `draw_rect` calls with the same `Paint` value share one `Arc<Paint>`.
pub(crate) fn interning_shares_arc_for_identical_paints() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &paint);
    canvas.draw_rect(Rect::from_ltrb(20.0, 20.0, 30.0, 30.0), &paint);

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();
    assert_eq!(cmds.len(), 2);
    assert!(
        Arc::ptr_eq(rect_paint(cmds[0]), rect_paint(cmds[1])),
        "identical paints must share one Arc allocation"
    );
}

/// A taken recording leaves the canvas empty at the identity transform, and
/// the next recording shares the paint allocations of the last unless the
/// paint holds a shader.
pub(crate) fn a_taken_recording_shares_its_plain_paints_with_the_next() {
    use flui_foundation::geometry::{Matrix4, Offset};
    use flui_painting::Shader;
    use flui_painting::paint::TileMode;

    let plain = Paint::fill(Color::RED);
    let mut shaded = Paint::fill(Color::RED);
    shaded.shader = Some(Shader::linear_gradient(
        Offset::ZERO,
        Offset::new(10.0, 0.0),
        vec![Color::RED, Color::BLUE],
        None,
        TileMode::Clamp,
    ));
    let rect = Rect::from_ltrb(0.0, 0.0, 10.0, 10.0);
    let mut canvas = Canvas::new();
    canvas.translate(5.0, 5.0);
    canvas.draw_rect(rect, &plain);
    canvas.draw_rect(rect, &shaded);
    let first = canvas.take_recording();
    assert_eq!(first.len(), 2);
    assert!(canvas.is_empty());
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());

    canvas.draw_rect(rect, &plain);
    canvas.draw_rect(rect, &shaded);
    let second = canvas.finish();
    assert_eq!(
        second.bounds(),
        Some(rect),
        "the translation did not carry over"
    );
    let (first, second): (Vec<&DrawCommand>, Vec<&DrawCommand>) = (
        first.commands().iter().collect(),
        second.commands().iter().collect(),
    );
    assert!(
        Arc::ptr_eq(rect_paint(first[0]), rect_paint(second[0])),
        "a plain paint is interned across recordings"
    );
    assert!(
        !Arc::ptr_eq(rect_paint(first[1]), rect_paint(second[1])),
        "a paint with a shader is let go with its recording"
    );
}

/// A canvas kept between recordings lets go of the room a recording that
/// interned many paints or saved deeply grew, so one heavy frame does not pin
/// its memory for every frame after it.
pub(crate) fn a_taken_recording_lets_go_of_a_heavy_frames_room() {
    let rect = Rect::from_ltrb(0.0, 0.0, 10.0, 10.0);
    let mut canvas = Canvas::new();
    for shade in 0..=u8::MAX {
        canvas.save();
        canvas.draw_rect(rect, &Paint::fill(Color::rgb(shade, 0, 0)));
    }
    for _ in 0..256 {
        canvas.restore();
    }
    let (paints, saves) = canvas.kept_room();
    assert!(paints >= 256 && saves >= 256, "the heavy frame grew both");
    drop(canvas.take_recording());
    let (paints, saves) = canvas.kept_room();
    assert!(paints <= 4, "the paint pool kept room for {paints} paints");
    assert!(saves <= 32, "the save stack kept room for {saves} states");
}

/// A finished `DisplayList` moves to another thread (the GPU hand-off) intact.
pub(crate) fn a_finished_display_list_is_sendable_to_another_thread() {
    let mut canvas = Canvas::new();
    for i in 0..100 {
        let rect = Rect::from_ltrb(f64::from(i), 0.0, f64::from(i + 1), 50.0);
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
    }
    let display_list = canvas.finish();
    let executed = thread::spawn(move || display_list.commands().iter().count())
        .join()
        .expect("the consumer thread must not panic");
    assert_eq!(executed, 100);
}

/// What a refused op leaves in the list (ADR-0306 §4).
#[derive(Clone, Copy, Debug)]
enum Refusal {
    /// A draw: absent.
    Omitted,
    /// A clip: an intersection with an empty rectangle, so what it would have
    /// clipped draws nothing.
    EmptyClip,
    /// A save-layer: opened unbounded, its restore still paired.
    Unbounded,
}

type Record = Box<dyn Fn(&mut Canvas)>;

/// One row per recorded variant: a call holding one non-finite value.
fn non_finite_rows() -> Vec<(&'static str, Record, Refusal)> {
    use flui_foundation::geometry::{Matrix4, Offset, Point, RRect, RSuperellipse};
    use flui_painting::paint::{
        ColorFilter, ColorMatrix, DashPattern, FilterQuality, Image, ImageRepeat, Path, TextureId,
        TileMode,
    };
    use flui_painting::{BlendMode, PointMode, Shader};

    const NAN: f64 = f64::NAN;
    let fill = || Paint::fill(Color::RED);
    let rect = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
    let bad = Rect::from_xywh(NAN, 0.0, 10.0, 10.0);
    let image = || Image::from_rgba8(1, 1, vec![255; 4]);
    let paragraph = shaped_paragraph();
    let shadow_paragraph = Arc::clone(&paragraph);
    let mut nan_path = Path::new();
    nan_path.move_to(Point::new(0.0, 0.0));
    nan_path.line_to(Point::new(NAN, 4.0));
    let clip_path = nan_path.clone();
    let shadow_path = nan_path.clone();
    let rows: Vec<(&'static str, Record, Refusal)> = vec![
        (
            "clip_rect",
            Box::new(move |c| c.clip_rect(bad)),
            Refusal::EmptyClip,
        ),
        (
            "clip_rrect",
            Box::new(move |c| c.clip_rrect(RRect::from_rect_circular(rect, NAN))),
            Refusal::EmptyClip,
        ),
        (
            "clip_rsuperellipse",
            Box::new(move |c| c.clip_rsuperellipse(RSuperellipse::from_rect_circular(bad, 2.0))),
            Refusal::EmptyClip,
        ),
        (
            "clip_path",
            Box::new(move |c| c.clip_path(&clip_path)),
            Refusal::EmptyClip,
        ),
        (
            "line",
            Box::new(move |c| c.draw_line(Point::new(0.0, 0.0), Point::new(NAN, 1.0), &fill())),
            Refusal::Omitted,
        ),
        (
            "rect",
            Box::new(move |c| c.draw_rect(bad, &fill())),
            Refusal::Omitted,
        ),
        (
            "rrect",
            Box::new(move |c| c.draw_rrect(RRect::from_rect_xy(rect, 2.0, NAN), &fill())),
            Refusal::Omitted,
        ),
        (
            "circle",
            Box::new(move |c| c.draw_circle(Point::new(5.0, 5.0), NAN, &fill())),
            Refusal::Omitted,
        ),
        (
            "oval",
            Box::new(move |c| c.draw_oval(bad, &fill())),
            Refusal::Omitted,
        ),
        (
            "path",
            Box::new(move |c| c.draw_path(&nan_path, &fill())),
            Refusal::Omitted,
        ),
        (
            "paragraph",
            Box::new(move |c| c.draw_paragraph(&paragraph, Offset::new(NAN, 0.0), Color::BLACK)),
            Refusal::Omitted,
        ),
        (
            "paragraph_shadow",
            Box::new(move |c| {
                c.draw_paragraph_shadow(
                    &shadow_paragraph,
                    Offset::new(0.0, NAN),
                    Color::BLACK,
                    flui_painting::BlurSigma::ZERO,
                );
            }),
            Refusal::Omitted,
        ),
        (
            "image",
            Box::new(move |c| c.draw_image(image(), bad, None)),
            Refusal::Omitted,
        ),
        (
            "image_region",
            Box::new(move |c| c.draw_image_region(image(), bad, rect, None)),
            Refusal::Omitted,
        ),
        (
            "image_repeat",
            Box::new(move |c| c.draw_image_repeat(image(), bad, ImageRepeat::Repeat, None)),
            Refusal::Omitted,
        ),
        (
            "image_nine_slice",
            Box::new(move |c| c.draw_image_nine_slice(image(), bad, rect, None)),
            Refusal::Omitted,
        ),
        (
            "image_filtered",
            Box::new(move |c| {
                let mut values = [0.0_f32; 20];
                values[0] = f32::NAN;
                c.draw_image_filtered(
                    image(),
                    rect,
                    ColorFilter::Matrix(ColorMatrix::new(values)),
                    None,
                );
            }),
            Refusal::Omitted,
        ),
        (
            "texture",
            Box::new(move |c| {
                c.draw_texture(TextureId::new(1), rect, None, FilterQuality::None, NAN);
            }),
            Refusal::Omitted,
        ),
        (
            "shadow",
            Box::new(move |c| c.draw_shadow(&shadow_path, Color::BLACK, 4.0)),
            Refusal::Omitted,
        ),
        (
            "arc",
            Box::new(move |c| c.draw_arc(rect, 0.0, NAN, false, &fill())),
            Refusal::Omitted,
        ),
        (
            "drrect",
            Box::new(move |c| {
                c.draw_drrect(
                    RRect::from_rect(rect),
                    RRect::from_rect(Rect::from_xywh(2.0, 2.0, NAN, 4.0)),
                    &fill(),
                );
            }),
            Refusal::Omitted,
        ),
        (
            "points",
            Box::new(move |c| {
                c.draw_points_with_mode(PointMode::Points, vec![Point::new(NAN, 0.0)], &fill());
            }),
            Refusal::Omitted,
        ),
        (
            "vertices",
            Box::new(move |c| {
                c.draw_vertices(
                    vec![
                        Point::new(0.0, 0.0),
                        Point::new(4.0, 0.0),
                        Point::new(0.0, 4.0),
                    ],
                    None,
                    Some(vec![
                        Point::new(0.0, 0.0),
                        Point::new(NAN, 0.0),
                        Point::new(0.0, 1.0),
                    ]),
                    vec![0, 1, 2],
                    &fill(),
                );
            }),
            Refusal::Omitted,
        ),
        (
            "paint (stroke width)",
            Box::new(move |c| {
                let mut paint = fill();
                paint.stroke_width = NAN;
                c.draw_paint(&paint);
            }),
            Refusal::Omitted,
        ),
        (
            "paint (dash phase)",
            Box::new(move |c| {
                let mut paint = Paint::stroke(Color::RED, 1.0);
                paint.dash_pattern = Some(DashPattern::new(vec![2.0, 2.0], NAN));
                c.draw_rect(rect, &paint);
            }),
            Refusal::Omitted,
        ),
        (
            "paint (shader stop)",
            Box::new(move |c| {
                let mut paint = fill();
                paint.shader = Some(Shader::linear_gradient(
                    Offset::ZERO,
                    Offset::new(10.0, 0.0),
                    vec![Color::RED, Color::BLUE],
                    Some(vec![0.0, NAN]),
                    TileMode::Clamp,
                ));
                c.draw_rect(rect, &paint);
            }),
            Refusal::Omitted,
        ),
        (
            "atlas",
            Box::new(move |c| {
                let mut transform = Matrix4::IDENTITY;
                transform.m[12] = NAN;
                c.draw_atlas(
                    image(),
                    vec![Rect::from_xywh(0.0, 0.0, 1.0, 1.0)],
                    vec![transform],
                    None,
                    BlendMode::SrcOver,
                    None,
                );
            }),
            Refusal::Omitted,
        ),
        (
            "save_layer",
            Box::new(move |c| {
                c.save_layer(Some(bad), &fill());
                c.restore();
            }),
            Refusal::Unbounded,
        ),
    ];
    rows
}

/// A laid-out paragraph to record, from the bundled faces.
fn shaped_paragraph() -> Arc<flui_painting::ShapedParagraph> {
    use flui_foundation::geometry::Offset;
    use flui_painting::typography::{TextDirection, TextSpan};
    use flui_painting::{FontCollection, TextContext, TextPainter};

    let fonts = FontCollection::new();
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Ag"))
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut TextContext::new(&fonts), 0.0, 200.0)
        .expect("valid fixture lays out");
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records its paragraph")
}

/// Whether every value the list reports about itself is finite.
fn extents_are_finite(list: &DisplayList) -> bool {
    let bounds_finite = list.bounds().is_none_or(|rect| rect.is_finite());
    let damage_finite = match list.damage_extent() {
        None | Some(flui_painting::DamageExtent::Unbounded) => true,
        Some(flui_painting::DamageExtent::Bounded { rect, spread }) => {
            rect.is_finite() && spread.is_finite()
        }
    };
    bounds_finite && damage_finite
}

/// Every recorded variant holding a non-finite value is refused by its kind,
/// and so is every op under a non-finite transform: a draw is absent, a clip
/// is empty, a save-layer is unbounded, the scope markers remain, the list's
/// extents stay finite and `non_finite()` counts the reason (ADR-0306 §4).
pub(crate) fn a_non_finite_op_is_refused_by_kind() {
    use flui_painting::paint::ClipOp;

    let mut failures = Vec::new();
    for (name, record, refusal) in non_finite_rows() {
        let list = flui_painting::testing::record(|canvas| {
            canvas.save();
            record(canvas);
            canvas.restore();
        });
        let ops: Vec<&DrawOp> = list.iter().map(|command| &command.op).collect();
        let kept = match refusal {
            Refusal::Omitted => matches!(ops.as_slice(), [DrawOp::Save, DrawOp::Restore]),
            Refusal::EmptyClip => matches!(
                ops.as_slice(),
                [
                    DrawOp::Save,
                    DrawOp::ClipRect {
                        rect,
                        clip_op: ClipOp::Intersect,
                        ..
                    },
                    DrawOp::Restore,
                ] if *rect == Rect::ZERO
            ),
            Refusal::Unbounded => matches!(
                ops.as_slice(),
                [
                    DrawOp::Save,
                    DrawOp::SaveLayer { bounds: None, .. },
                    DrawOp::RestoreLayer,
                    DrawOp::Restore,
                ]
            ),
        };
        let counted = list.non_finite().geometry == 1 && list.non_finite().transform == 0;
        if !kept || !counted || !extents_are_finite(&list) {
            failures.push(format!("{name}: {ops:?}, {:?}", list.non_finite()));
        }
    }
    assert!(failures.is_empty(), "refused by kind: {failures:#?}");

    // Under a non-finite transform: the draw, the clip and the save-layer are
    // refused for the transform, and the markers stay, under the identity.
    let bounded = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
    let list = flui_painting::testing::record(|canvas| {
        canvas.save();
        canvas.translate(f64::NAN, 0.0);
        canvas.draw_rect(bounded, &Paint::fill(Color::RED));
        canvas.clip_rect(bounded);
        canvas.save_layer(Some(bounded), &Paint::fill(Color::WHITE));
        canvas.restore();
        canvas.restore();
        canvas.draw_rect(bounded, &Paint::fill(Color::BLUE));
    });
    let ops: Vec<&DrawOp> = list.iter().map(|command| &command.op).collect();
    assert!(
        matches!(
            ops.as_slice(),
            [
                DrawOp::Save,
                DrawOp::ClipRect { rect, .. },
                DrawOp::SaveLayer { bounds: None, .. },
                DrawOp::RestoreLayer,
                DrawOp::Restore,
                DrawOp::Rect { .. },
            ] if *rect == Rect::ZERO
        ),
        "a non-finite transform refuses by kind: {ops:?}"
    );
    assert!(
        list.iter()
            .all(|command| command.transform.m.iter().all(|value| value.is_finite())),
        "no recorded transform is non-finite"
    );
    assert_eq!(list.non_finite().transform, 3);
    assert_eq!(list.non_finite().geometry, 0);
    assert_eq!(list.bounds(), Some(bounded), "only the finite draw counts");
    assert!(extents_are_finite(&list));

    // `append` sums the counts.
    let mut combined = DisplayList::new();
    combined.append_isolated(list.clone());
    combined.append(list);
    assert_eq!(combined.non_finite().transform, 6);

    // A run whose every op was refused is empty, and an isolated append
    // still counts it.
    let refused = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(
            Rect::from_ltrb(0.0, 0.0, f64::NAN, 10.0),
            &Paint::fill(Color::RED),
        );
    });
    assert!(refused.is_empty());
    combined.append_isolated(refused);
    assert_eq!(combined.non_finite().geometry, 1);
    assert_eq!(combined.non_finite().transform, 6);
}

/// Font bytes whose destructor panics: user code a recorded paragraph's last
/// owner runs.
struct PanicOnDrop(&'static [u8]);

impl AsRef<[u8]> for PanicOnDrop {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("font bytes dropped");
    }
}

/// `Canvas::reset` resets every field before it drops what it recorded, so a
/// destructor that panics leaves an empty canvas with no cached extent, an
/// identity transform and no open save, and the canvas records again
/// (ADR-0306 §7).
pub(crate) fn a_destructor_that_panics_during_reset_leaves_the_canvas_empty() {
    use flui_foundation::geometry::Offset;

    let bytes: flui_painting::glyphs::FontBytes = Arc::new(PanicOnDrop(b""));
    let paragraph = Arc::new(flui_painting::testing::paragraph_with_font_bytes(
        &shaped_paragraph(),
        &bytes,
    ));
    drop(bytes);
    let mut canvas = Canvas::new();
    canvas.save();
    canvas.translate(7.0, 9.0);
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
        &Paint::fill(Color::RED),
    );
    canvas.draw_paragraph(&paragraph, Offset::ZERO, Color::BLACK);
    drop(paragraph);

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| canvas.reset()));
    assert!(
        unwound.is_err(),
        "the recorded paragraph was the bytes' last owner"
    );
    assert!(canvas.is_empty());
    assert_eq!(canvas.bounds(), None);
    assert_eq!(canvas.display_list().damage_extent(), None);
    assert_eq!(canvas.display_list().non_finite(), NonFiniteOps::default());
    assert_eq!(canvas.save_count(), 1);
    assert_eq!(
        canvas.transform_matrix(),
        flui_foundation::geometry::Matrix4::IDENTITY
    );

    let rect = Rect::from_xywh(1.0, 2.0, 3.0, 4.0);
    canvas.draw_rect(rect, &Paint::fill(Color::RED));
    assert_eq!(
        canvas.finish().bounds(),
        Some(rect),
        "the canvas records again"
    );
}
