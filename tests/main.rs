//! Integration coverage for the shared benchmark application.
#[path = "../examples/benchmark_grid/correctness.rs"]
mod correctness;
#[path = "../examples/benchmark_grid/measurement.rs"]
mod measurement;
#[path = "../examples/benchmark_grid/scene.rs"]
mod scene;

mod benchmark_grid {
    use super::{correctness::*, measurement::*, scene::*};
    use flui_foundation::geometry::Offset;
    use flui_testing::{FrameReport, PointerScript};

    #[test]
    fn grid_has_every_item_and_local_change_preserves_body() {
        for kind in [Kind::Labels, Kind::Buttons] {
            let scene = GridScene::new(Shape::for_items(100).expect("fixture"), kind);
            let mut binding = mount(&scene);
            verify_scene(&binding, &scene, "Benchmark 0").expect("scene contract");
            verify_input(&mut binding, &scene).expect("input contract");
            let before = paragraphs(&binding);
            assert_eq!(before.len(), scene.shape.items() + 1);
            assert_eq!(
                before.iter().filter(|text| text.as_str() == "X").count(),
                80
            );
            assert!(before.iter().any(|text| text == "Item 19"));
            let local = sample(&mut binding, &scene, Mode::Local, 1);
            let after = paragraphs(&binding);
            assert!(after.iter().any(|text| text == "Benchmark 1"));
            assert_eq!(
                before
                    .iter()
                    .filter(|text| !text.starts_with("Benchmark"))
                    .collect::<Vec<_>>(),
                after
                    .iter()
                    .filter(|text| !text.starts_with("Benchmark"))
                    .collect::<Vec<_>>()
            );
            let retained = sample(&mut binding, &scene, Mode::Retained, 2);
            let rebuild = sample(&mut binding, &scene, Mode::Rebuild, 3);
            let count = |sample: &Sample, name| {
                sample
                    .counters
                    .iter()
                    .find(|(key, _)| *key == name)
                    .expect("counter")
                    .1
            };
            assert_eq!(count(&retained, "elements_rebuilt"), 0);
            assert_eq!(count(&retained, "nodes_laid_out"), 0);
            assert!(count(&local, "elements_rebuilt") > 0);
            assert!(
                count(&local, "nodes_painted") <= 20,
                "static body must reuse paint"
            );
            assert!(count(&rebuild, "elements_rebuilt") > count(&local, "elements_rebuilt"));
            if kind == Kind::Buttons {
                binding.replay(&PointerScript::tap(Offset::new(20.0, 80.0)));
                assert_eq!(scene.activated.get(), Some(0));
                binding.replay(&PointerScript::tap(Offset::new(280.0, 68.0)));
                assert_eq!(scene.activated.get(), Some(20));
            }
        }
    }

    #[test]
    fn wall_clock_includes_work_and_percentiles_use_nearest_rank() {
        let sample = measure(|| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            FrameReport::default()
        });
        assert!(sample.elapsed_ns >= 5_000_000);
        let values: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&values, 50), 50);
        assert_eq!(percentile(&values, 95), 95);
        assert_eq!(percentile(&values, 99), 99);
        assert_eq!(percentile(&[7], 99), 7);
        assert!(Shape::for_items(101).is_none());
    }

    #[test]
    fn pixel_oracle_rejects_blank_missing_and_stale_captures() {
        let shape = Shape::for_items(100).expect("fixture");
        assert!(verify_pixels(&[], shape).is_err());
        let blank = vec![0; WIDTH as usize * HEIGHT as usize * 4];
        assert!(verify_pixels(&blank, shape).is_err());
        assert!(verify_header_delta(&blank, &blank).is_err());
    }
}
