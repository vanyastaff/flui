//! Handoff observes the sampled simulation derivative, not its policy hint.

use super::*;
use flui_animation::Simulation;
use flui_animation::simulation::{FrictionSimulation, Tolerance};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Default)]
struct Samples {
    time: AtomicU64,
    derivative: AtomicU64,
}

#[derive(Debug)]
struct MeasuredPhysics(Arc<Samples>);

impl flui_widgets::ScrollPhysics for MeasuredPhysics {
    fn apply_boundary_conditions(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        proposed: f64,
    ) -> f64 {
        proposed.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    }
    fn create_ballistic_simulation(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity: f64,
    ) -> Option<Box<dyn Simulation>> {
        if velocity == 0.0 {
            return None;
        }
        Some(Box::new(MeasuredSimulation {
            source: FrictionSimulation::new(
                0.135,
                metrics.pixels,
                velocity,
                Tolerance::for_device_pixel_ratio(metrics.device_pixel_ratio).ok()?,
            )
            .ok()?,
            samples: self.0.clone(),
        }))
    }
    fn boundary_velocity(&self, _: &flui_widgets::ScrollMetrics, velocity: f64) -> Option<f64> {
        // Transfer eligibility is deliberately distinct from the real dx(t).
        (velocity != 0.0).then_some(7777.0)
    }
}

struct MeasuredSimulation {
    source: FrictionSimulation,
    samples: Arc<Samples>,
}
impl Simulation for MeasuredSimulation {
    fn x(&self, time: f64) -> f64 {
        let value = self.source.x(time);
        self.samples.time.store(time.to_bits(), Ordering::Relaxed);
        value
    }
    fn dx(&self, time: f64) -> f64 {
        let value = self.source.dx(time);
        self.samples
            .derivative
            .store(value.to_bits(), Ordering::Relaxed);
        value
    }
    fn is_done(&self, time: f64) -> bool {
        self.source.is_done(time)
    }
}

struct Capture {
    samples: Arc<Samples>,
    offers: Rc<RefCell<Vec<(FlingStep, f64, f64)>>>,
}
impl NestedScrollConnection for Capture {
    fn post_fling(&self, step: &FlingStep) -> flui_widgets::FlingHandoff {
        self.offers.borrow_mut().push((
            *step,
            f64::from_bits(self.samples.time.load(Ordering::Relaxed)),
            f64::from_bits(self.samples.derivative.load(Ordering::Relaxed)),
        ));
        flui_widgets::FlingHandoff::Declined
    }
}

pub(crate) fn three_level_handoff_keeps_identity_and_uses_the_actual_sampled_derivative() {
    let samples = Arc::new(Samples::default());
    let offers = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::new(RefCell::new(Vec::new()));
    let outer = ScrollController::new();
    let middle = ScrollController::new();
    let inner = ScrollController::with_initial_scroll_offset(350.0);
    let vsync = flui_animation::Vsync::new();
    let leaf = NestedScroll::new(Rc::new(Capture {
        samples: samples.clone(),
        offers: offers.clone(),
    }))
    .child(
        Scrollable::new()
            .controller(inner.clone())
            .physics(Arc::new(MeasuredPhysics(samples)))
            .child(SizedBox::new(100.0, 600.0)),
    );
    let middle_view = Scrollable::new()
        .controller(middle.clone())
        .child(SizedBox::new(100.0, 100.0).child(leaf));
    let outer_view = Scrollable::new().controller(outer.clone()).child(
        SizedBox::new(100.0, 1000.0).child(
            flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT)
                .child(SizedBox::new(100.0, 100.0).child(middle_view)),
        ),
    );
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(Rc::new(MotionProbe(log.clone()))).child(outer_view),
        ),
        tight(100.0, 100.0),
    );
    laid.adopt_vsync(vsync).expect("owner clock");
    laid.pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    laid.dispatch_pointer_down(50.0, 80.0);
    for y in [60.0, 40.0, 20.0] {
        laid.dispatch_pointer_move_after(50.0, y, Duration::from_millis(20));
    }
    laid.dispatch_pointer_up(50.0, 20.0);
    assert!(
        inner.pixels() < 500.0,
        "release starts inside the leaf extent"
    );
    for _ in 0..30 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(inner.pixels(), 500.0);
    assert_eq!(
        middle.pixels(),
        0.0,
        "a saturated owner declines continuation"
    );
    assert!(
        outer.pixels() > 0.0,
        "the next willing ancestor receives the motion"
    );
    let id = match log.borrow().first() {
        Some(MotionEvent::Started(id)) => *id,
        other => panic!("source: {other:?}"),
    };
    {
        let offers = offers.borrow();
        assert_eq!(
            offers.len(),
            1,
            "the old simulation cannot offer after handoff"
        );
        let (step, time, derivative) = offers[0];
        assert_eq!(step.sequence, id);
        assert_eq!(step.device_pixel_ratio, 2.0);
        assert!(time > 0.0 && derivative > 0.0 && derivative < 2000.0);
        assert!(
            (step.available.pixels_per_second.dy + derivative).abs() < 1e-8,
            "offer must be actual dx({time}), not the deliberately different boundary hint: {step:?}, dx={derivative}"
        );
    }
    laid.pump_for(Duration::from_secs(6));
    let log = log.borrow();
    let ends: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let MotionEvent::End(sequence, reason) = event {
                Some((*sequence, *reason))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(ends, [(id, ScrollEndReason::Released)]);
    assert!(
        log.iter()
            .filter_map(|event| if let MotionEvent::Step(step) = event {
                Some(step)
            } else {
                None
            })
            .all(|step| step.sequence == id)
    );
}
