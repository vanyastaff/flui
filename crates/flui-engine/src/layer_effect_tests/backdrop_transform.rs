//! A transform backdrop filter shows the backdrop magnified: the stripes it
//! covers come out wider, the ones beside it stay as they were.
use super::*;
use flui_foundation::geometry::Point;
use flui_layer::ImageFilterLayer;
use flui_painting::paint::{ImageSampling, ImageTransform};

/// Vertical stripes 4 px wide, black from x = 0, white from x = 4, and so on.
fn stripes() -> Layer {
    let mut canvas = Canvas::new();
    for stripe in 0..SIDE / 4 {
        let color = if stripe % 2 == 0 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        canvas.draw_rect(
            Rect::from_xywh(f64::from(stripe * 4), 0.0, 4.0, f64::from(SIDE)),
            &Paint::fill(color).with_anti_alias(false),
        );
    }
    Layer::from(PictureLayer::new(canvas.finish()))
}

/// The stripes under `filter` as a backdrop filter over 16..48 on both
/// axes, under `matrix`.
fn scene(matrix: Matrix4, filter: ImageFilter) -> LayerTree {
    let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
    tree.push_child(tree.root(), stripes());
    tree.push_child(
        tree.root(),
        Layer::from(BackdropFilterLayer::new(
            filter,
            BlendMode::Src,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
        )),
    );
    tree
}

const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

pub(super) fn magnified_backdrop_doubles_the_stripes_it_covers(renderer: &HeadlessRenderer) {
    let magnify = ImageFilter::try_magnify(Point::new(32.0, 32.0), 2.0).expect("a valid scale");
    let plain = capture(renderer, &scene(Matrix4::IDENTITY, ImageFilter::blur(0.0)));
    let magnified = capture(renderer, &scene(Matrix4::IDENTITY, magnify.clone()));
    dump_named("backdrop-transform-magnified", &magnified);
    // Inside the loupe pixel x shows the backdrop at (x + 32) / 2, so the
    // stripe 24..28 fills 16..24 and the stripe 28..32 fills 24..32.
    for (x, expected) in [(18, BLACK), (22, BLACK), (25, WHITE), (30, WHITE)] {
        expect_pixel(&magnified, (x, 32), expected);
    }
    // The same pixels unmagnified: 22 is white and 25 black, so the
    // witnesses above tell magnified from plain.
    expect_pixel(&plain, (22, 32), WHITE);
    expect_pixel(&plain, (25, 32), BLACK);
    // Beside the loupe the stripes stay.
    for (x, expected) in [(10, BLACK), (13, WHITE), (54, WHITE), (57, BLACK)] {
        expect_pixel(&magnified, (x, 32), expected);
    }

    // Under a layer transform the magnifier is in the layer's space: moved
    // 8 px right, the loupe covers 24..56 and its focal point is at 40, so
    // device pixel 34 shows the layer's (34.5 - 8 + 32) / 2 = 29.25, in the
    // white stripe 28..32. Magnified about device 32 instead it would show
    // 33.25 - 8 = 25.25, and unmagnified 26.5: both black.
    let moved = capture(
        renderer,
        &scene(Matrix4::translation(8.0, 0.0, 0.0), magnify.clone()),
    );
    expect_pixel(&moved, (34, 32), WHITE);

    // A rotated map has no lowering: the frame fails instead of drawing the
    // backdrop untransformed.
    let rotated = Matrix4::translation(32.0, 32.0, 0.0)
        * Matrix4::rotation_z(0.3)
        * Matrix4::translation(-32.0, -32.0, 0.0);
    let turned = ImageFilter::Transform {
        transform: ImageTransform::try_new(
            rotated.m[0],
            rotated.m[1],
            rotated.m[4],
            rotated.m[5],
            rotated.m[12],
            rotated.m[13],
        )
        .expect("a rotation is invertible"),
        sampling: ImageSampling::Linear,
    };
    assert!(matches!(
        renderer.render_layer_tree(&scene(Matrix4::IDENTITY, turned), (SIDE, SIDE)),
        Err(crate::EngineError::UnsupportedBackdropFilter { .. })
    ));
    let nearest = ImageFilter::Transform {
        transform: ImageTransform::try_scale_about(Point::new(32.0, 32.0), 2.0, 2.0)
            .expect("a valid scale"),
        sampling: ImageSampling::Nearest,
    };
    assert!(matches!(
        renderer.render_layer_tree(&scene(Matrix4::IDENTITY, nearest), (SIDE, SIDE)),
        Err(crate::EngineError::UnsupportedBackdropFilter { .. })
    ));
    // On a layer's own content a transform filter fails the frame too.
    let mut content = LayerTree::new(Layer::from(ImageFilterLayer::new(magnify)));
    content.push_child(content.root(), stripes());
    assert!(matches!(
        renderer.render_layer_tree(&content, (SIDE, SIDE)),
        Err(crate::EngineError::UnsupportedImageFilter { .. })
    ));
    // The next frame draws again.
    expect_pixel(
        &capture(renderer, &scene(Matrix4::IDENTITY, ImageFilter::blur(0.0))),
        (22, 32),
        WHITE,
    );
}
