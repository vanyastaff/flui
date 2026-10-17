//! Image placement: fit, alignment and repetition in logical units, and a
//! nine-slice whose corners keep their logical size.

use flui_foundation::geometry::{Rect, Size};
use flui_painting::paint::{Image, ImageRepeat};
use flui_painting::{Alignment, BoxFit, Canvas, DrawOp, ImagePlacement};

/// One recorded image region: source texels, destination, tile, repeat.
type Region = (Rect<f64>, Rect<f64>, Option<Rect<f64>>, ImageRepeat);

fn image(width: u32, height: u32) -> Image {
    Image::from_rgba8(
        width,
        height,
        [255, 0, 0, 255].repeat((width * height) as usize),
    )
}

fn regions(
    placement: ImagePlacement,
    rect: Rect<f64>,
    image: &Image,
    natural: Size<f64>,
) -> Vec<Region> {
    let mut canvas = Canvas::new();
    placement.paint(&mut canvas, rect, image, natural, None, None);
    canvas
        .finish()
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::ImageRegion {
                src,
                dst,
                tile,
                repeat,
                ..
            } => Some((*src, *dst, *tile, *repeat)),
            _ => None,
        })
        .collect()
}

fn ltrb(left: f64, top: f64, right: f64, bottom: f64) -> Rect<f64> {
    Rect::from_ltrb(left, top, right, bottom)
}

/// A 2x-scale image keeps half-size logical corners; the edges stretch along
/// their side and the centre takes the rest.
pub(crate) fn sliced_corners_keep_their_logical_size() {
    let placement = ImagePlacement::sliced(ltrb(1.0, 1.0, 3.0, 3.0));
    let rect = ltrb(0.0, 0.0, 20.0, 10.0);
    let found = regions(placement, rect, &image(4, 4), Size::new(2.0, 2.0));
    let pairs: Vec<_> = found.iter().map(|(src, dst, ..)| (*src, *dst)).collect();
    assert_eq!(
        pairs,
        [
            (ltrb(0.0, 0.0, 1.0, 1.0), ltrb(0.0, 0.0, 0.5, 0.5)),
            (ltrb(1.0, 0.0, 3.0, 1.0), ltrb(0.5, 0.0, 19.5, 0.5)),
            (ltrb(3.0, 0.0, 4.0, 1.0), ltrb(19.5, 0.0, 20.0, 0.5)),
            (ltrb(0.0, 1.0, 1.0, 3.0), ltrb(0.0, 0.5, 0.5, 9.5)),
            (ltrb(1.0, 1.0, 3.0, 3.0), ltrb(0.5, 0.5, 19.5, 9.5)),
            (ltrb(3.0, 1.0, 4.0, 3.0), ltrb(19.5, 0.5, 20.0, 9.5)),
            (ltrb(0.0, 3.0, 1.0, 4.0), ltrb(0.0, 9.5, 0.5, 10.0)),
            (ltrb(1.0, 3.0, 3.0, 4.0), ltrb(0.5, 9.5, 19.5, 10.0)),
            (ltrb(3.0, 3.0, 4.0, 4.0), ltrb(19.5, 9.5, 20.0, 10.0)),
        ]
    );
    assert!(
        found
            .iter()
            .all(|(.., tile, repeat)| tile.is_none() && *repeat == ImageRepeat::NoRepeat)
    );
    assert_eq!(
        placement.destination(rect, Size::new(4.0, 4.0), Size::new(2.0, 2.0)),
        Some(rect)
    );
}

/// Corners wider than the box shrink together on that axis, never
/// overlapping; the other axis keeps its size.
pub(crate) fn sliced_corners_shrink_together_in_a_small_box() {
    let found = regions(
        ImagePlacement::sliced(ltrb(2.0, 2.0, 4.0, 4.0)),
        ltrb(0.0, 0.0, 2.0, 10.0),
        &image(6, 6),
        Size::new(6.0, 6.0),
    );
    let destinations: Vec<_> = found.iter().map(|(_, dst, ..)| *dst).collect();
    // Horizontal borders total 4 in a 2-wide box: each shrinks to 1 and the
    // centre column is empty, so it is not drawn.
    assert_eq!(
        destinations,
        [
            ltrb(0.0, 0.0, 1.0, 2.0),
            ltrb(1.0, 0.0, 2.0, 2.0),
            ltrb(0.0, 2.0, 1.0, 8.0),
            ltrb(1.0, 2.0, 2.0, 8.0),
            ltrb(0.0, 8.0, 1.0, 10.0),
            ltrb(1.0, 8.0, 2.0, 10.0),
        ]
    );
}

/// A centre outside the decoded texels, inverted or non-finite paints
/// nothing and has no destination.
pub(crate) fn an_invalid_slice_paints_nothing() {
    let rect = ltrb(0.0, 0.0, 10.0, 10.0);
    for center in [
        ltrb(0.0, 0.0, 5.0, 3.0),
        ltrb(-1.0, 0.0, 2.0, 2.0),
        Rect::from_ltrb(3.0, 1.0, 1.0, 3.0),
        ltrb(1.0, 1.0, f64::NAN, 3.0),
    ] {
        let placement = ImagePlacement::sliced(center);
        assert!(
            regions(placement, rect, &image(4, 4), Size::new(4.0, 4.0)).is_empty(),
            "{center:?}"
        );
        assert_eq!(
            placement.destination(rect, Size::new(4.0, 4.0), Size::new(4.0, 4.0)),
            None
        );
    }
}

/// `FitWidth` and `FitHeight` crop the other axis, and the alignment picks
/// the visible part of the source.
pub(crate) fn fit_width_and_height_crop_at_the_alignment() {
    let tall = image(10, 20);
    let natural = Size::new(10.0, 20.0);
    let square = ltrb(0.0, 0.0, 20.0, 20.0);
    for (fit, alignment, src, dst) in [
        (
            BoxFit::FitWidth,
            Alignment::TOP_CENTER,
            ltrb(0.0, 0.0, 10.0, 10.0),
            square,
        ),
        (
            BoxFit::FitWidth,
            Alignment::BOTTOM_CENTER,
            ltrb(0.0, 10.0, 10.0, 20.0),
            square,
        ),
        (
            BoxFit::FitHeight,
            Alignment::CENTER,
            ltrb(0.0, 0.0, 10.0, 20.0),
            ltrb(5.0, 0.0, 15.0, 20.0),
        ),
    ] {
        let found = regions(
            ImagePlacement::fitted(fit, alignment),
            square,
            &tall,
            natural,
        );
        assert_eq!(found.len(), 1, "{fit:?}");
        assert_eq!(
            (found[0].0, found[0].1),
            (src, dst),
            "{fit:?} at {alignment:?}"
        );
    }
}

/// A fractional alignment places a letterboxed image between the edges.
pub(crate) fn a_fractional_alignment_places_between_edges() {
    let placement = ImagePlacement::fitted(BoxFit::Contain, Alignment::new(0.5, -1.0));
    let rect = ltrb(0.0, 0.0, 100.0, 50.0);
    let found = regions(placement, rect, &image(10, 10), Size::new(10.0, 10.0));
    // The 50x50 image leaves 50 of width; x = 0.5 sits three quarters along.
    assert_eq!(found[0].1, ltrb(37.5, 0.0, 87.5, 50.0));
    assert_eq!(
        placement.destination(rect, Size::new(10.0, 10.0), Size::new(10.0, 10.0)),
        Some(ltrb(37.5, 0.0, 87.5, 50.0))
    );
}

/// Repetition tiles the image at its logical size, so a 2x asset repeats
/// every five logical pixels from its aligned phase.
pub(crate) fn repetition_tiles_at_the_logical_size() {
    let placement = ImagePlacement::Fitted {
        fit: BoxFit::ScaleDown,
        alignment: Alignment::TOP_LEFT,
        repeat: ImageRepeat::Repeat,
    };
    let rect = ltrb(0.0, 0.0, 20.0, 20.0);
    let found = regions(placement, rect, &image(10, 10), Size::new(5.0, 5.0));
    assert_eq!(
        found,
        [(
            ltrb(0.0, 0.0, 10.0, 10.0),
            rect,
            Some(ltrb(0.0, 0.0, 5.0, 5.0)),
            ImageRepeat::Repeat
        )]
    );
}
