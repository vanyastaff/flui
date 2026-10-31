//! The choreographer's next frame as the clock the Android loop runs its
//! frames on (ADR-0269).
//!
//! A turn with a frame to deliver posts one callback for the choreographer's
//! next frame and blocks until the looper is woken; the callback records the
//! frame's vsync, and on API 33 when that frame is expected on screen, and
//! wakes the looper; the turn whose poll ran the callback delivers the frame,
//! stamped with them ([`VsyncTick::timing`]). A tick no turn delivers (the
//! Activity paused during the poll, the request gone) is dropped and the next
//! frame posts again, so a frame never runs on a stale vsync.
//!
//! The callback runs on the loop's thread, inside `poll_events`:
//! `ALooper_pollOnce` runs the choreographer's file-descriptor callback and
//! goes on waiting, which is why the callback wakes the looper. A posted frame
//! callback cannot be taken back, so each post hands it a counted reference to
//! the state it records into, which the callback releases; one still posted
//! when the loop exits is leaked with that state.

#![cfg_attr(target_os = "android", expect(unsafe_code))]

use std::time::Duration;

/// What the choreographer reported for a frame, on `CLOCK_MONOTONIC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VsyncTick {
    /// The frame's vsync.
    pub(crate) frame_time_nanos: i64,
    /// When the frame's preferred timeline expects it on screen (API 33).
    pub(crate) expected_present_nanos: Option<i64>,
}

impl VsyncTick {
    /// The frame this tick stands for, read at `now_nanos`, as
    /// `(age, until_present)`: its vsync is `age` before the reading, and it
    /// reaches the screen `until_present` after it (ADR-0257). A vsync ahead of
    /// the reading is the reading itself, and an expected presentation that is
    /// not after both the vsync and the reading is unknown.
    #[must_use]
    pub(crate) fn timing(self, now_nanos: i64) -> (Duration, Option<Duration>) {
        let after = |earlier: i64, later: i64| {
            u64::try_from(later.saturating_sub(earlier))
                .ok()
                .map(Duration::from_nanos)
        };
        let age = after(self.frame_time_nanos, now_nanos).unwrap_or_default();
        let until_present = self
            .expected_present_nanos
            .filter(|present| *present > self.frame_time_nanos)
            .and_then(|present| after(now_nanos, present))
            .filter(|until| !until.is_zero());
        (age, until_present)
    }
}

/// How a frame the loop delivers is stamped.
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameStamp {
    /// With the choreographer's tick, where it paces the frames.
    Vsync(VsyncTick),
    /// With this reading of the event clock, where nothing does.
    Now,
}

#[cfg(target_os = "android")]
pub(crate) use clock::{FrameClock, monotonic_nanos};

#[cfg(target_os = "android")]
mod clock {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use android_activity::AndroidAppWaker;
    use flui_platform::FrameRequest;
    use parking_lot::Mutex;

    use super::{FrameStamp, VsyncTick};
    use crate::owner::android::ndk::{
        AChoreographer, AChoreographerFrameCallbackData, FrameCallbacks, FramePost,
    };
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// What a posted callback shares with the loop.
    struct Slot {
        /// A callback is posted and has not run.
        posted: AtomicBool,
        /// What the last callback that ran reported, until the loop takes it.
        tick: Mutex<Option<VsyncTick>>,
        /// Ends the poll the callback ran in.
        waker: AndroidAppWaker,
        /// How a callback is posted, and how a vsync callback's data is read.
        post: FramePost,
    }

    impl Slot {
        fn record(&self, tick: VsyncTick) {
            *self.tick.lock() = Some(tick);
            self.posted.store(false, Ordering::SeqCst);
            self.waker.wake();
        }
    }

    /// The loop thread's choreographer, which paces its frames. Not `Send`: a
    /// choreographer serves the thread it belongs to.
    pub(crate) struct FrameClock {
        choreographer: NonNull<AChoreographer>,
        slot: Arc<Slot>,
    }

    impl FrameClock {
        /// The calling thread's choreographer, posting with `callbacks` and
        /// waking the looper through `waker`; `None` when the thread has none.
        pub(crate) fn start(callbacks: FrameCallbacks, waker: AndroidAppWaker) -> Option<Self> {
            // SAFETY: `AChoreographer_getInstance` takes nothing and returns
            // the calling thread's instance, or null on a thread without a
            // looper.
            let choreographer = NonNull::new(unsafe { (callbacks.instance.get())() })?;
            Some(Self {
                choreographer,
                slot: Arc::new(Slot {
                    posted: AtomicBool::new(false),
                    tick: Mutex::new(None),
                    waker,
                    post: callbacks.post,
                }),
            })
        }

        /// Ask for a callback at the choreographer's next frame, unless one is
        /// posted already.
        pub(crate) fn arm(&self) {
            if self.slot.posted.swap(true, Ordering::SeqCst) {
                return;
            }
            let data = Arc::into_raw(Arc::clone(&self.slot))
                .cast_mut()
                .cast::<c_void>();
            let choreographer = self.choreographer.as_ptr();
            // SAFETY: `choreographer` is this thread's (`FrameClock` is not
            // `Send`), each post is the NDK function of its name, and `data` is
            // the counted reference its callback releases once.
            unsafe {
                match &self.slot.post {
                    FramePost::Vsync { post, .. } => (post.get())(choreographer, on_vsync, data),
                    FramePost::Frame64(post) => (post.get())(choreographer, on_frame, data),
                    #[cfg(target_pointer_width = "64")]
                    FramePost::Frame(post) => (post.get())(choreographer, on_frame, data),
                }
            }
        }

        /// What the callback that ran in the last poll reported, taken: a tick
        /// serves the turn whose poll ran it, or none.
        pub(crate) fn take(&self) -> Option<FrameStamp> {
            self.slot.tick.lock().take().map(FrameStamp::Vsync)
        }
    }

    impl FrameStamp {
        /// The frame request this stamp makes, read now.
        pub(crate) fn request(self) -> FrameRequest {
            match self {
                Self::Vsync(tick) => {
                    let (age, until_present) = tick.timing(monotonic_nanos());
                    crate::frame_request_timed(age, until_present)
                }
                Self::Now => crate::frame_request_now(),
            }
        }
    }

    /// `CLOCK_MONOTONIC`, the clock the choreographer stamps its frames on.
    #[cfg_attr(
        target_pointer_width = "64",
        expect(
            clippy::useless_conversion,
            reason = "the fields are 32 bits on a 32-bit process"
        )
    )]
    pub(crate) fn monotonic_nanos() -> i64 {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid `timespec` for the call to fill;
        // `CLOCK_MONOTONIC` is always present on Android.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut now) };
        i64::from(now.tv_sec)
            .saturating_mul(1_000_000_000)
            .saturating_add(i64::from(now.tv_nsec))
    }

    /// The frame callback: the choreographer's next frame began at
    /// `frame_time_nanos`.
    unsafe extern "C" fn on_frame(frame_time_nanos: i64, data: *mut c_void) {
        os_entry(
            "the choreographer's frame callback",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                // SAFETY: `data` is the counted reference `FrameClock::arm`
                // posted with this callback, released here once.
                let slot = unsafe { Arc::from_raw(data.cast_const().cast::<Slot>()) };
                slot.record(VsyncTick {
                    frame_time_nanos,
                    expected_present_nanos: None,
                });
            },
        );
    }

    /// The vsync callback: the choreographer's next frame, with its frame
    /// timelines.
    unsafe extern "C" fn on_vsync(
        frame: *const AChoreographerFrameCallbackData,
        data: *mut c_void,
    ) {
        os_entry(
            "the choreographer's vsync callback",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                // SAFETY: as in `on_frame`.
                let slot = unsafe { Arc::from_raw(data.cast_const().cast::<Slot>()) };
                let FramePost::Vsync {
                    frame_time,
                    preferred,
                    expected_present,
                    ..
                } = &slot.post
                else {
                    unreachable!("BUG: only the vsync post registers the vsync callback");
                };
                // SAFETY: `frame` is the data the choreographer hands this
                // callback, valid while it runs; each reading is the NDK
                // function of its name, and the index is the one the data
                // names as preferred.
                let tick = unsafe {
                    let index = (preferred.get())(frame);
                    VsyncTick {
                        frame_time_nanos: (frame_time.get())(frame),
                        expected_present_nanos: Some((expected_present.get())(frame, index)),
                    }
                };
                slot.record(tick);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::VsyncTick;

    #[test]
    fn a_tick_places_its_vsync_and_presentation_around_the_reading() {
        let ms = Duration::from_millis;
        const MS: i64 = 1_000_000;
        // (case, now, frame time, expected present, age, until_present)
        type Row = (
            &'static str,
            i64,
            i64,
            Option<i64>,
            Duration,
            Option<Duration>,
        );
        let rows: &[Row] = &[
            (
                "a tick read after its vsync",
                1_002 * MS,
                1_000 * MS,
                Some(1_016 * MS),
                ms(2),
                Some(ms(14)),
            ),
            (
                "a frame callback, which names no presentation",
                1_002 * MS,
                1_000 * MS,
                None,
                ms(2),
                None,
            ),
            (
                "a vsync ahead of the reading",
                999 * MS,
                1_000 * MS,
                Some(1_016 * MS),
                ms(0),
                Some(ms(17)),
            ),
            (
                "a presentation already passed",
                1_020 * MS,
                1_000 * MS,
                Some(1_016 * MS),
                ms(20),
                None,
            ),
            (
                "a presentation not after the vsync",
                1_002 * MS,
                1_000 * MS,
                Some(1_000 * MS),
                ms(2),
                None,
            ),
            (
                "a distance that overflows the clock",
                i64::MAX,
                i64::MIN,
                Some(i64::MAX),
                Duration::from_nanos(9_223_372_036_854_775_807),
                None,
            ),
        ];
        for &(case, now, frame_time_nanos, expected_present_nanos, age, until_present) in rows {
            let tick = VsyncTick {
                frame_time_nanos,
                expected_present_nanos,
            };
            assert_eq!(tick.timing(now), (age, until_present), "{case}");
        }
    }
}
