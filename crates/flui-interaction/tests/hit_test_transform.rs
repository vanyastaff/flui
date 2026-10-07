use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset};
use flui_interaction::{HitTestEntry, HitTestResult};

fn local_point(result: &HitTestResult, index: usize, global: (f64, f64)) -> (f64, f64) {
    result.path()[index]
        .transform
        .expect("entry carries its delivery transform")
        .transform_point(global.0, global.1)
}

fn assert_point(actual: (f64, f64), expected: (f64, f64)) {
    assert!(
        (actual.0 - expected.0).abs() < 1e-10,
        "x: {actual:?} != {expected:?}"
    );
    assert!(
        (actual.1 - expected.1).abs() < 1e-10,
        "y: {actual:?} != {expected:?}"
    );
}

fn refused_transform(transform: Matrix4) {
    let called = Cell::new(false);
    let mut result = HitTestResult::new();
    let _ = result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
        let outcome = result.with_paint_transform(transform, |result| {
            called.set(true);
            result.add(HitTestEntry::new(RenderId::new(1)));
            true
        });
        assert_eq!(outcome, None);
        assert!(!called.get(), "refused subtree must not execute");
        assert!(
            result.is_empty(),
            "refused subtree must not publish entries"
        );
        let hit = result.with_paint_transform(Matrix4::translation(3.0, 4.0, 0.0), |result| {
            result.add(HitTestEntry::new(RenderId::new(2)));
            true
        });
        assert_eq!(hit, Some(true));
        assert_point(local_point(result, 0, (15.0, 27.0)), (2.0, 3.0));
    });
    result.add(HitTestEntry::new(RenderId::new(3)));
    assert_point(local_point(&result, 1, (15.0, 27.0)), (15.0, 27.0));
}

fn singular_paint_transform_skips_the_callback() {
    refused_transform(Matrix4::scaling(0.0, 1.0, 1.0));
}

fn nan_paint_transform_skips_the_callback() {
    refused_transform(Matrix4::translation(f64::NAN, 0.0, 0.0));
}

fn infinite_paint_transform_skips_the_callback() {
    refused_transform(Matrix4::scaling(f64::INFINITY, 1.0, 1.0));
}

fn inverse_overflow_skips_the_callback() {
    refused_transform(Matrix4::scaling(f64::from_bits(1), 1.0, 1.0));
}

fn determinant_overflow_skips_the_callback() {
    refused_transform(Matrix4::from([
        1e100, 0.0, 0.0, 0.0, 0.0, 1e100, 0.0, 0.0, 0.0, 0.0, 1e100, 0.0, 0.0, 0.0, 0.0, 1e100,
    ]));
}

fn refused_offset(offset: Offset) {
    let called = Cell::new(false);
    let mut result = HitTestResult::new();
    let _ = result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
        // Refusal is observed through the subtree, not the scope's return
        // value, so the contract holds whatever shape that value takes.
        let _ = result.with_paint_offset(offset, |result| {
            called.set(true);
            result.add(HitTestEntry::new(RenderId::new(1)));
        });
        assert!(!called.get(), "refused subtree must not execute");
        assert!(
            result.is_empty(),
            "refused subtree must not publish entries"
        );
        result.add(HitTestEntry::new(RenderId::new(2)));
        assert_point(local_point(result, 0, (15.0, 27.0)), (5.0, 7.0));
    });
}

fn nan_paint_offset_skips_the_callback() {
    refused_offset(Offset::new(f64::NAN, 0.0));
}

fn infinite_paint_offset_skips_the_callback() {
    refused_offset(Offset::new(0.0, f64::NEG_INFINITY));
}

fn tiny_invertible_paint_transform_maps_local_coordinates() {
    let mut result = HitTestResult::new();
    let hit = result.with_paint_transform(Matrix4::scaling(1e-9, 1e-9, 1.0), |result| {
        result.add(HitTestEntry::new(RenderId::new(1)));
        true
    });
    assert_eq!(hit, Some(true));
    assert_point(local_point(&result, 0, (2e-9, 3e-9)), (2.0, 3.0));
}

fn paint_transform_unwind_restores_the_next_scope() {
    let mut result = HitTestResult::new();
    let _ = result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
        let failure: Result<Option<()>, _> = catch_unwind(AssertUnwindSafe(|| {
            result.with_paint_transform(Matrix4::scaling(2.0, 3.0, 1.0), |result| {
                result.add(HitTestEntry::new(RenderId::new(1)));
                panic!("descendant failed");
            })
        }));
        let payload = failure.expect_err("descendant failure must propagate");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"descendant failed"));
        result
            .with_paint_transform(Matrix4::translation(3.0, 4.0, 0.0), |result| {
                result.add(HitTestEntry::new(RenderId::new(2)));
            })
            .expect("healthy sibling transform must be admitted");
        assert_point(local_point(result, 1, (15.0, 27.0)), (2.0, 3.0));
    });
    result.add(HitTestEntry::new(RenderId::new(3)));
    assert_point(local_point(&result, 2, (15.0, 27.0)), (15.0, 27.0));
}

fn refused_callback_retirement_failure_preserves_the_next_scope() {
    struct Capture<'a>(&'a Cell<usize>);

    impl Drop for Capture<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("refused capture retired");
        }
    }

    let called = Cell::new(false);
    let retirements = Cell::new(0);
    let mut result = HitTestResult::new();
    let _ = result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
        let capture = Capture(&retirements);
        let called_ref = &called;
        let failure = catch_unwind(AssertUnwindSafe(|| {
            result.with_paint_transform(Matrix4::scaling(0.0, 1.0, 1.0), move |_| {
                called_ref.set(true);
                drop(capture);
                true
            })
        }));
        let payload = failure.expect_err("refused callback capture retirement must propagate");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"refused capture retired")
        );
        assert!(!called.get(), "refusal must not invoke the callback body");
        assert_eq!(retirements.get(), 1);
        assert!(result.is_empty());
        result
            .with_paint_transform(Matrix4::translation(3.0, 4.0, 0.0), |result| {
                result.add(HitTestEntry::new(RenderId::new(1)));
            })
            .expect("healthy sibling must remain admissible after capture retirement failure");
        assert_point(local_point(result, 0, (15.0, 27.0)), (2.0, 3.0));
    });
    result.add(HitTestEntry::new(RenderId::new(2)));
    assert_point(local_point(&result, 1, (15.0, 27.0)), (15.0, 27.0));
}

fn perspective_delivery(
    transform: Matrix4,
    positions: [(f64, f64); 3],
    expected: Option<[(f64, f64); 3]>,
) {
    use std::rc::Rc;
    use flui_foundation::geometry::Point;
    use flui_interaction::InteractionLane;
    use flui_platform_api::{EventTime, pointer::{PointerEvent, PointerId, PointerInfo,
        PointerKind, PointerButtons, PointerMove, PointerPosition, PointerSample, Pressure}};

    let sample = |index: usize| PointerSample::new(EventTime::from_nanos(index as u64 + 1),
        PointerPosition::try_new(Point::new(positions[index].0, positions[index].1))
            .expect("finite screen sample"))
        .with_pressure(Pressure::try_new(0.4).expect("valid pressure"));
    let pointer = PointerInfo::new(PointerId::try_from(1_u64).expect("nonzero"), PointerKind::Touch);
    let event = PointerEvent::Move(PointerMove::new(pointer, PointerButtons::NONE, sample(1))
        .with_coalesced(vec![sample(0)]).with_predicted(vec![sample(2)]));
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let deliveries = Rc::new(Cell::new(0));
    lane.enter(|| {
        let observed = deliveries.clone();
        let source = event.clone();
        let target = handle.register_pointer(move |dispatch| {
            let expected = expected.expect("invalid projection must not deliver");
            assert_eq!(dispatch.global, &source, "the complete source stays unchanged");
            let PointerEvent::Move(local) = dispatch.local else { panic!("local Move") };
            let PointerEvent::Move(global) = dispatch.global else { panic!("global Move") };
            assert_eq!(local.pointer, global.pointer);
            assert_eq!(local.buttons, global.buttons);
            assert_eq!(local.modifiers, global.modifiers);
            for ((local, global), point) in [
                (&local.coalesced()[0], &global.coalesced()[0]),
                (local.current(), global.current()),
                (&local.predicted()[0], &global.predicted()[0]),
            ].into_iter().zip(expected) {
                assert_point((local.position.get().x, local.position.get().y), point);
                assert_eq!(local.time, global.time);
                assert_eq!(local.pressure, global.pressure);
            }
            observed.set(observed.get() + 1);
        }).expect("register pointer");
        let mut result = HitTestResult::new();
        result.with_paint_transform(transform, |result| {
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        }).expect("finite invertible fixture");
        let route = handle.resolve_pointer_route(result.path()).expect("resolve route").token();
        let failure = handle.invoke_pointer_route(route, &event).expect("invoke route");
        if let Some(failure) = failure { failure.resume(); }
        assert_eq!(deliveries.get(), usize::from(expected.is_some()));
        handle.release_route(route).expect("release route");
    });
}

fn perspective_delivery_preserves_the_source_and_unprojects_samples() {
    let transform = Matrix4::from([
        0.6, 0.0, -0.8, -0.08, 0.0, 1.0, 0.0, 0.0,
        0.8, 0.0, 0.6, 0.06, 0.0, 0.0, 0.0, 1.0,
    ]);
    let local = [(1.0, 2.0), (2.0, 3.0), (3.0, 4.0)];
    // Analytic forward projection of the z=0 plane, independent of the inverse helper.
    let screen = local.map(|(x, y)| (0.6 * x / (1.0 - 0.08 * x), y / (1.0 - 0.08 * x)));
    perspective_delivery(transform, screen, Some(local));
}

fn perspective_delivery_refuses_hidden_and_degenerate_planes() {
    for (transform, point) in [
        (Matrix4::from([
            -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0,
        ]), (2.0, 3.0)),
        (Matrix4::from([
            1.0, 0.0, 0.0, -0.5, 0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]), (-2.0, 3.0)),
        (Matrix4::from([
            1.0, 0.0, 0.0, -0.5, 0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]), (-2.0 + f64::EPSILON, 3.0)),
        (Matrix4::from([
            0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]), (2.0, 3.0)),
    ] {
        perspective_delivery(transform, [point; 3], None);
    }
}

fn plane_unprojection_preserves_tiny_affine_delivery() {
    perspective_delivery(Matrix4::scaling(1e-9, 1e-9, 1.0),
        [(1e-9, 2e-9), (2e-9, 3e-9), (3e-9, 4e-9)],
        Some([(1.0, 2.0), (2.0, 3.0), (3.0, 4.0)]));
}

fn plane_unprojection_preserves_admitted_anisotropic_delivery() {
    for z_scale in [1.0, 1e200] {
        let transform = Matrix4::scaling(1e-200, 1.0, z_scale);
        assert!(transform.try_inverse().is_some(), "checked inversion admits z_scale={z_scale}");
        perspective_delivery(transform,
            [(1e-200, 2.0), (2e-200, 3.0), (3e-200, 4.0)],
            Some([(1.0, 2.0), (2.0, 3.0), (3.0, 4.0)]));
    }
}

#[test]
fn hit_test_transform_admission() {
    let mut failures = Vec::new();
    for (name, case) in [
        (
            "admitted anisotropic sample families",
            plane_unprojection_preserves_admitted_anisotropic_delivery as fn(),
        ),
        (
            "tiny affine sample families",
            plane_unprojection_preserves_tiny_affine_delivery as fn(),
        ),
        (
            "perspective sample families",
            perspective_delivery_preserves_the_source_and_unprojects_samples as fn(),
        ),
        (
            "perspective refusal",
            perspective_delivery_refuses_hidden_and_degenerate_planes as fn(),
        ),
        (
            "singular",
            singular_paint_transform_skips_the_callback as fn(),
        ),
        ("NaN", nan_paint_transform_skips_the_callback as fn()),
        (
            "infinite",
            infinite_paint_transform_skips_the_callback as fn(),
        ),
        (
            "inverse overflow",
            inverse_overflow_skips_the_callback as fn(),
        ),
        (
            "determinant overflow",
            determinant_overflow_skips_the_callback as fn(),
        ),
        (
            "tiny finite scale",
            tiny_invertible_paint_transform_maps_local_coordinates as fn(),
        ),
        (
            "refused capture retirement",
            refused_callback_retirement_failure_preserves_the_next_scope as fn(),
        ),
        (
            "descendant unwind",
            paint_transform_unwind_restores_the_next_scope as fn(),
        ),
    ] {
        if let Err(payload) = catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "hit transform cases failed: {failures:?}"
    );
}

#[test]
fn hit_test_offset_admission() {
    let mut failures = Vec::new();
    for (name, case) in [
        ("NaN offset", nan_paint_offset_skips_the_callback as fn()),
        (
            "infinite offset",
            infinite_paint_offset_skips_the_callback as fn(),
        ),
    ] {
        if let Err(payload) = catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "hit offset cases failed: {failures:?}");
}
