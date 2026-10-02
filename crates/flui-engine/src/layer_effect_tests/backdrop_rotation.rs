//! Public right-angle matrices keep anisotropic Gaussian axes meaningful.
use super::*;

fn scene(matrix: Matrix4, source: Rect<f64>, sigma: (f64, f64)) -> LayerTree {
    let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
    tree.push_child(
        tree.root(),
        picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::BLACK),
    );
    tree.push_child(tree.root(), picture(source, Color::WHITE));
    tree.push_child(
        tree.root(),
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::Blur {
                sigma_x: sigma.0,
                sigma_y: sigma.1,
            },
            BlendMode::Src,
            Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
        )),
    );
    tree
}

pub(super) fn quarter_turn_backdrops_match_baked_axes(renderer: &HeadlessRenderer) {
    let local_source = Rect::from_xywh(28.0, 22.0, 4.0, 10.0);
    for (angle, baked_source, sigma, halo) in [
        (
            std::f64::consts::FRAC_PI_2,
            Rect::from_xywh(32.0, 28.0, 10.0, 4.0),
            (1.0, 4.0),
            (37, 25),
        ),
        (
            -std::f64::consts::FRAC_PI_2,
            Rect::from_xywh(22.0, 32.0, 10.0, 4.0),
            (1.0, 4.0),
            (27, 29),
        ),
        (
            std::f64::consts::PI,
            Rect::from_xywh(32.0, 32.0, 4.0, 10.0),
            (4.0, 1.0),
            (29, 37),
        ),
    ] {
        let matrix = Matrix4::translation(32.0, 32.0, 0.0)
            * Matrix4::rotation_z(angle)
            * Matrix4::translation(-32.0, -32.0, 0.0);
        let expected = capture(renderer, &scene(Matrix4::IDENTITY, baked_source, sigma));
        let actual = capture(renderer, &scene(matrix, local_source, (4.0, 1.0)));
        assert!(
            pixel(&expected, halo.0, halo.1)[0] > 5,
            "anisotropic halo witness"
        );
        dump_named(&format!("backdrop-rotation-{angle}-actual"), &actual);
        dump_named(&format!("backdrop-rotation-{angle}-expected"), &expected);
        let mismatch = actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .enumerate()
            .find(|(_, (a, b))| a.iter().zip(*b).any(|(&a, &b)| a.abs_diff(b) > 2));
        assert!(
            mismatch.is_none(),
            "public rotation_z({angle}) differs at {mismatch:?}"
        );
    }
    // A real rotation still requires a directional kernel, not axis snapping.
    let skewed = Matrix4::translation(32.0, 32.0, 0.0)
        * Matrix4::rotation_z(0.01)
        * Matrix4::translation(-32.0, -32.0, 0.0);
    assert!(matches!(
        renderer.render_layer_tree(&scene(skewed, local_source, (4.0, 1.0)), (SIDE, SIDE)),
        Err(crate::EngineError::UnsupportedBackdropFilter { .. })
    ));
    expect_pixel(
        &capture(
            renderer,
            &LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN)),
        ),
        (16, 16),
        [0, 255, 0, 255],
    );
}
