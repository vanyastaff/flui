//! A frame dropped on a lost GPU device waits out the dropped-frame backoff,
//! and a given-up device rebuild holds the presentation's frames until the
//! application asks for a new one (ADR-0267).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_runtime::frame_failure::{
    DeviceRecovery, FailureDisposition, FrameFailureHandler, FrameFailureKind,
};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::UiRuntime;

/// A sink whose device is lost until the test says otherwise.
struct Device {
    lost: bool,
    submits: usize,
}

impl FrameSink for Device {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        self.submits += 1;
        if self.lost {
            SubmitVerdict::DeviceLost
        } else {
            SubmitVerdict::Presented
        }
    }
}

struct Lost {
    runtime: UiRuntime,
    wakes: Arc<AtomicUsize>,
    reports: Arc<parking_lot::Mutex<Vec<(DeviceRecovery, FailureDisposition)>>>,
    device: Device,
    now: web_time::Instant,
}

impl Lost {
    /// A mounted runtime whose first frame was dropped on a lost device.
    fn new() -> Self {
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&wakes);
        let runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        runtime
            .attach_root_widget_with_size(&flui_widgets::Text::new("lost"), 400.0, 400.0)
            .expect("mount text");
        let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen = Arc::clone(&reports);
        runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            let FrameFailureKind::DeviceLost { recovery, .. } = report.kind else {
                panic!("only device loss fails here: {:?}", report.kind);
            };
            seen.lock().push((recovery, report.disposition));
        })));
        let mut lost = Self {
            runtime,
            wakes,
            reports,
            device: Device {
                lost: true,
                submits: 0,
            },
            now: web_time::Instant::now(),
        };
        assert!(!lost.pump(), "the lost device shows nothing");
        assert_eq!(lost.device.submits, 1, "the first frame reached the device");
        lost
    }

    fn pump(&mut self) -> bool {
        self.now += std::time::Duration::from_millis(16);
        self.runtime
            .pump(
                &mut flui_runtime::pump::SampledClock(self.now),
                &mut self.device,
            )
            .presented()
    }

    /// Sleep until the dropped frame's retry deadline, as a host woken at it.
    fn wait_out_the_backoff(&self) {
        let deadline = self
            .runtime
            .failure_retry_deadline()
            .expect("a dropped frame waits out its backoff");
        std::thread::sleep(deadline.saturating_duration_since(web_time::Instant::now()));
        assert!(self.runtime.has_pending_work(), "the frame is due again");
    }

    fn reports(&self) -> Vec<(DeviceRecovery, FailureDisposition)> {
        self.reports.lock().clone()
    }

    /// Whether the runtime asks its host for another frame, by wake or by
    /// pending work.
    fn wants_a_frame(&self, wakes_before: usize) -> bool {
        self.wakes.load(Ordering::SeqCst) > wakes_before || self.runtime.has_pending_work()
    }
}

#[test]
fn a_frame_dropped_on_a_lost_device_waits_out_the_backoff() {
    let lost = Lost::new();
    assert_eq!(
        lost.reports(),
        vec![(DeviceRecovery::Pending, FailureDisposition::FrameDropped)],
        "the dropped frame is reported once, before any rebuild failed"
    );
    assert!(
        lost.runtime.failure_retry_deadline().is_some(),
        "the frame is tried again after the dropped-frame backoff"
    );
    assert!(
        !lost.runtime.needs_redraw(),
        "no frame is asked for before the backoff runs out"
    );
    assert!(
        !lost.runtime.has_pending_work(),
        "the repaint waits for the deadline"
    );
    assert_eq!(
        lost.runtime.next_wake(),
        lost.runtime.failure_retry_deadline(),
        "the host sleeps until the retry deadline"
    );
}

#[test]
fn a_failed_rebuild_is_reported_with_the_dropped_frame() {
    let mut lost = Lost::new();
    lost.runtime
        .note_primary_device_recovery(Some(DeviceRecovery::Failed { attempts: 3 }));
    lost.wait_out_the_backoff();
    assert!(!lost.pump());
    assert_eq!(
        lost.reports().last(),
        Some(&(
            DeviceRecovery::Failed { attempts: 3 },
            FailureDisposition::FrameDropped
        )),
    );
}

#[test]
fn a_given_up_rebuild_holds_frames_until_the_application_asks_again() {
    let mut lost = Lost::new();
    lost.runtime
        .note_primary_device_recovery(Some(DeviceRecovery::Abandoned { attempts: 17 }));
    assert_eq!(
        lost.reports().last(),
        Some(&(
            DeviceRecovery::Abandoned { attempts: 17 },
            FailureDisposition::FrameDropped
        )),
        "giving up is reported"
    );
    let reported = lost.reports().len();
    let wakes = lost.wakes.load(Ordering::SeqCst);
    assert_eq!(
        lost.runtime.failure_retry_deadline(),
        None,
        "no retry is waited out"
    );
    assert!(
        !lost.wants_a_frame(wakes),
        "a given-up device asks for no frame"
    );

    // A frame some other source asks for still reaches the dead device, and
    // neither reports again nor arms a retry.
    lost.runtime.wake_frame();
    assert!(!lost.pump());
    assert_eq!(lost.reports().len(), reported, "giving up is reported once");
    assert_eq!(lost.runtime.failure_retry_deadline(), None);
    let wakes = lost.wakes.load(Ordering::SeqCst);
    assert!(!lost.wants_a_frame(wakes));

    assert!(lost.runtime.retry_render_device(), "a rebuild is asked for");
    assert_eq!(
        lost.runtime.primary_device_recovery(),
        Some(DeviceRecovery::Pending)
    );
    assert!(
        lost.wants_a_frame(wakes),
        "the asked-for rebuild asks for a frame"
    );
    assert!(
        !lost.runtime.retry_render_device(),
        "a rebuild already asked for is not asked again"
    );

    lost.runtime.note_primary_device_recovery(None);
    lost.device.lost = false;
    let submits = lost.device.submits;
    assert!(lost.pump(), "the rebuilt device shows the whole tree");
    assert_eq!(lost.device.submits, submits + 1);
}
