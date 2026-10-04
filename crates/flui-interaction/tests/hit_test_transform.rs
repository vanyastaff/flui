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
    result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
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
    result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
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
    result.with_paint_offset(Offset::new(10.0, 20.0), |result| {
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

#[test]
fn hit_test_transform_admission() {
    let mut failures = Vec::new();
    for (name, case) in [
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
