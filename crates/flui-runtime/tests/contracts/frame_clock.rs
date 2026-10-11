//! The host-selected timestamp owns all animation samples in one pump.

use flui_animation::{Animation, AnimationController, Vsync};
use flui_foundation::{Listenable, ManualClock, MonotonicClock};
use flui_runtime::pump::{FrameClockSource, SampledClock};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

struct AdvancingHostClock {
    selected: web_time::Instant,
    host: ManualClock,
    reads: usize,
}

impl FrameClockSource for AdvancingHostClock {
    fn frame_time(&mut self) -> web_time::Instant {
        self.reads += 1;
        self.host.advance(Duration::from_secs(2));
        self.selected
    }
}

struct Sink;

impl FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

#[test]
fn one_pump_shares_its_selected_time_across_parent_and_child() {
    let host = ManualClock::new();
    let mut runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(host.clone()),
        ),
    )
    .expect("manual-clock runtime");
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::square(10.0), 800.0, 600.0)
        .expect("mounted root");
    let registry = runtime.vsync();
    let child = Vsync::new();
    registry.attach_child(&child).expect("nested registry");
    let parent_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let child_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let parent_run = parent_owner.controller().forward().expect("parent run");
    let child_run = child_owner.controller().forward().expect("child run");
    let _ = runtime.pump(&mut SampledClock(host.now()), &mut Sink);

    // Represent host time spent in an earlier animation callback without a
    // wall-clock sleep. No new pump is admitted by this callback.
    let armed = Rc::new(Cell::new(true));
    let observed = Rc::clone(&armed);
    let callback_clock = host.clone();
    let listener = child_owner.controller().add_listener(Rc::new(move || {
        if observed.replace(false) {
            callback_clock.advance(Duration::from_secs(3));
        }
    }));
    host.advance(Duration::from_millis(250));
    let selected = host.now();
    let mut frame = AdvancingHostClock {
        selected,
        host: host.clone(),
        reads: 0,
    };
    let _ = runtime.pump(&mut frame, &mut Sink);
    assert_eq!(frame.reads, 1, "the pump selects its timestamp once");
    assert!(!armed.get(), "the child callback ran in this pump");
    assert_eq!(host.now().duration_since(selected), Duration::from_secs(5));
    for (name, controller, run) in [
        ("parent", parent_owner.controller(), &parent_run),
        ("child", child_owner.controller(), &child_run),
    ] {
        assert!(
            (controller.value() - 0.25).abs() < 1e-9,
            "{name} samples the selected frame time, not the later host time"
        );
        assert!(run.is_pending(), "{name} retains its unfinished run");
    }

    let _ = runtime.pump(
        &mut SampledClock(selected + Duration::from_millis(250)),
        &mut Sink,
    );
    assert!((parent_owner.controller().value() - 0.5).abs() < 1e-9);
    assert!((child_owner.controller().value() - 0.5).abs() < 1e-9);
    child_owner.controller().remove_listener(listener);
}
