//! The AppKit host under test: its event loop, run by hand on the main
//! thread where a test harness stands in for `MacOSPlatform::run`.

use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

/// How long one wait for an event lasts, so the loop looks at its deadline
/// between events.
const EVENT_WAIT: Duration = Duration::from_millis(20);

/// Run AppKit for `span`: the events it delivers, the main queue the owner
/// lane posts to, and the animations a mode change plays.
///
/// What `MacOSPlatform::run` does on each turn of its loop, for a test that
/// owns the main thread instead. The application must have finished
/// launching.
pub fn run_for(mtm: MainThreadMarker, span: Duration) {
    let app = NSApplication::sharedApplication(mtm);
    #[expect(
        unsafe_code,
        reason = "`NSDefaultRunLoopMode` is a Foundation extern static"
    )]
    // SAFETY: Foundation defines the constant and the loader initialises it
    // before any Rust code runs; nothing writes it afterwards.
    let mode = unsafe { NSDefaultRunLoopMode };
    let deadline = Instant::now() + span;
    while Instant::now() < deadline {
        let until = NSDate::dateWithTimeIntervalSinceNow(EVENT_WAIT.as_secs_f64());
        if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&until),
            mode,
            true,
        ) {
            app.sendEvent(&event);
        }
    }
}
