//! The `CAMetalLayer` the AppKit window's view is backed by, which tells the
//! window when each of its frames reached the screen.
//!
//! wgpu keeps the drawables it presents to itself, but it takes the view's
//! root layer when that is a `CAMetalLayer` and gets every drawable from its
//! `nextDrawable`. So the window's view is backed by [`FluiMetalLayer`], whose
//! `nextDrawable` hands each drawable a presented handler before wgpu sees it:
//! Metal calls it on a thread of its own with the time the drawable went on
//! screen, on the `CACurrentMediaTime` clock, and the handler reports that
//! time on the event clock to the window's [`PresentationReports`].
//!
//! The rebasing is pure and runs on every host ([`presented`]); the layer
//! class is AppKit's.

use std::time::Duration;

use flui_platform::{DisplayId, DisplayRevision, DisplaySnapshot, FramePresented, RefreshPeriod};

/// What a drawable shown at `presented_secs` reports, when `media_now_secs`
/// is a reading of the same clock taken as the event clock read `event_now`
/// (nanoseconds), on a display whose refresh is `refresh` (`None` when
/// variable or unknown). A time of 0 or less is Metal's "never shown", and a
/// time that is no finite number of seconds reports nothing either.
#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn presented(
    event_now: u64,
    media_now_secs: f64,
    presented_secs: f64,
    refresh: Option<Duration>,
) -> Option<FramePresented> {
    if presented_secs <= 0.0 {
        return None;
    }
    let stamp = Duration::try_from_secs_f64(presented_secs).ok()?;
    let now = Duration::try_from_secs_f64(media_now_secs).ok()?;
    let report = FramePresented::new(crate::owner::handlers::rebased_at(event_now, now, stamp));
    Some(match refresh {
        Some(refresh) => report.with_refresh(refresh),
        None => report,
    })
}

/// The refresh a layer is told its display has now: `None` when the display
/// has none, or the window is on no display.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewRefresh(pub(crate) Option<Duration>);

/// The refresh a window's layer reports carry, kept current from the display
/// snapshot: what was last pushed, by the snapshot revision and the display
/// it was read for.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
pub(crate) struct LayerRefresh {
    pushed: Option<(DisplayRevision, Option<DisplayId>)>,
}

#[cfg(any(target_os = "macos", test))]
impl LayerRefresh {
    /// What to push to the layer of a window on `display` under `snapshot`:
    /// the display's refresh period, `None` inside when it has none or the
    /// window is on no display; nothing to push when neither the snapshot nor
    /// the display moved since the last push. A rate change on the same
    /// display is a new snapshot revision, so it is pushed too.
    pub(crate) fn update(
        &mut self,
        snapshot: &DisplaySnapshot,
        display: Option<DisplayId>,
    ) -> Option<NewRefresh> {
        let read = (snapshot.revision(), display);
        if self.pushed == Some(read) {
            return None;
        }
        self.pushed = Some(read);
        Some(NewRefresh(
            display
                .and_then(|id| snapshot.get(id)?.refresh_period)
                .map(RefreshPeriod::get),
        ))
    }
}

#[cfg(target_os = "macos")]
pub(crate) use layer::{FluiMetalLayer, feedback_of};

#[cfg(target_os = "macos")]
mod layer {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{DefinedClass, define_class, msg_send};
    use objc2_foundation::NSObject;
    use objc2_metal::MTLDrawable;
    use objc2_quartz_core::{CACurrentMediaTime, CALayer, CAMetalDrawable, CAMetalLayer};

    use crate::owner::handlers::PresentationReports;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    /// What a layer reports to: its window's reports, set once the window
    /// has them, and the refresh interval of the display the window is on, in
    /// nanoseconds (0 when variable or unknown), which the window keeps
    /// current.
    #[derive(Debug, Default)]
    pub(crate) struct Feedback {
        reports: OnceLock<PresentationReports>,
        refresh_nanos: AtomicU64,
    }

    /// The presented handler holds the feedback on a Metal thread.
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Feedback>();
    };

    impl Feedback {
        /// Report to `reports` from now on; a layer reports to one window.
        pub(crate) fn report_to(&self, reports: PresentationReports) {
            if self.reports.set(reports).is_err() {
                tracing::warn!(
                    target: "flui.present",
                    "a Metal layer was told a second window to report to; it keeps the first"
                );
            }
        }

        /// The display under the window refreshes every `refresh`; `None`
        /// when its rate is variable or unknown.
        pub(crate) fn set_refresh(&self, refresh: Option<Duration>) {
            let nanos = refresh.map_or(0, |refresh| {
                u64::try_from(refresh.as_nanos()).unwrap_or(u64::MAX)
            });
            self.refresh_nanos.store(nanos, Ordering::Relaxed);
        }

        fn refresh(&self) -> Option<Duration> {
            match self.refresh_nanos.load(Ordering::Relaxed) {
                0 => None,
                nanos => Some(Duration::from_nanos(nanos)),
            }
        }

        /// Metal showed `drawable`, or skipped it.
        fn shown(&self, drawable: &ProtocolObject<dyn MTLDrawable>) {
            let Some(reports) = self.reports.get() else {
                return;
            };
            let presented_secs = drawable.presentedTime();
            let media_now_secs = CACurrentMediaTime();
            if let Some(presented) = super::presented(
                flui_platform::EventTime::now().as_nanos(),
                media_now_secs,
                presented_secs,
                self.refresh(),
            ) {
                tracing::trace!(
                    target: "flui.present",
                    presented_secs,
                    media_now_secs,
                    at_ns = presented.at.as_nanos(),
                    refresh_ns = presented.refresh.map(|refresh| refresh.as_nanos()),
                    "Metal reported a drawable shown"
                );
                reports.report(presented);
            } else {
                tracing::trace!(
                    target: "flui.present",
                    presented_secs,
                    "Metal reported a drawable never shown"
                );
            }
        }
    }

    define_class!(
        // SAFETY:
        // - `CAMetalLayer` has no subclassing requirements beyond its
        //   superclasses'; Core Animation subclasses it the same way.
        // - The class does not implement `Drop`; its ivars drop with it.
        // - Every instance has its ivars set before `CALayer`'s initializer
        //   runs: `init` sets fresh ones, and `initWithLayer:` shares the
        //   `Arc` of the layer Core Animation copies, so a presentation copy
        //   reports to the same window as its model layer.
        /// The layer an AppKit window's view is backed by: a `CAMetalLayer`
        /// whose drawables report when they reached the screen.
        #[unsafe(super(CAMetalLayer, CALayer, NSObject))]
        #[name = "FLUIMetalLayer"]
        #[ivars = Arc<Feedback>]
        pub(crate) struct FluiMetalLayer;

        // Every method runs its body in the panic boundary.
        impl FluiMetalLayer {
            /// `makeBackingLayer` makes the layer through `init`, which sets
            /// its ivars first. A panic there leaves a half-made layer, so the
            /// process ends.
            #[unsafe(method_id(init))]
            fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
                os_entry("-[FLUIMetalLayer init]", (), AfterPanic::Abort, || {
                    let this = this.set_ivars(Arc::default());
                    // SAFETY: `init` is `CALayer`'s designated initializer;
                    // the receiver has its ivars set above.
                    unsafe { msg_send![super(this), init] }
                })
            }

            /// Core Animation copies a layer for its presentation tree with
            /// `initWithLayer:`; the copy shares what the original reports to.
            #[unsafe(method_id(initWithLayer:))]
            fn init_with_layer(this: Allocated<Self>, layer: &AnyObject) -> Option<Retained<Self>> {
                os_entry("-[FLUIMetalLayer initWithLayer:]", (), AfterPanic::Abort, || {
                    let feedback = layer
                        .downcast_ref::<FluiMetalLayer>()
                        .map_or_else(Arc::default, |layer| Arc::clone(layer.ivars()));
                    let this = this.set_ivars(feedback);
                    // SAFETY: `initWithLayer:` is `CALayer`'s initializer for
                    // a copy; `layer` is the live layer Core Animation copies.
                    unsafe { msg_send![super(this), initWithLayer: layer] }
                })
            }

            /// Core Animation displays the layer: an AppKit view's display
            /// pass. It does what `CAMetalLayer`'s `display` does, then what
            /// AppKit's own backing layers add: they forward it to the view's
            /// `updateLayer`; `CAMetalLayer`'s `display` does not, and the
            /// view answers no `displayLayer:`, so a view backed by this
            /// layer would never be asked for a frame. This one asks the
            /// view, which answers `updateLayer`.
            #[unsafe(method(display))]
            fn display(&self) {
                os_entry(
                    "-[FLUIMetalLayer display]",
                    (),
                    AfterPanic::Answer(|()| ()),
                    || {
                        // SAFETY: `display` is `CAMetalLayer`'s, sent to this live
                        // layer; `delegate` answers the layer's delegate or nil;
                        // `updateLayer` takes no argument.
                        unsafe {
                            let _: () = msg_send![super(self), display];
                            let delegate: Option<Retained<AnyObject>> = msg_send![self, delegate];
                            if let Some(delegate) = delegate {
                                let updates: bool = msg_send![
                                    &*delegate,
                                    respondsToSelector: objc2::sel!(updateLayer)
                                ];
                                if updates {
                                    let _: () = msg_send![&*delegate, updateLayer];
                                }
                            }
                        }
                    },
                );
            }

            /// The next drawable, with a handler that reports when it is
            /// shown. A panic hands out no drawable, which wgpu takes as a
            /// timed-out acquire.
            #[unsafe(method_id(nextDrawable))]
            fn next_drawable(&self) -> Option<Retained<ProtocolObject<dyn CAMetalDrawable>>> {
                os_entry(
                    "-[FLUIMetalLayer nextDrawable]",
                    (),
                    AfterPanic::Answer(|()| None),
                    || {
                        // SAFETY: `nextDrawable` is `CAMetalLayer`'s; the
                        // receiver is this live layer.
                        let drawable: Option<Retained<ProtocolObject<dyn CAMetalDrawable>>> =
                            unsafe { msg_send![super(self), nextDrawable] };
                        if let Some(drawable) = &drawable {
                            watch(drawable, Arc::clone(self.ivars()));
                        }
                        drawable
                    },
                )
            }
        }
    );

    impl FluiMetalLayer {
        /// A new layer that reports nowhere until its window says where.
        pub(crate) fn new() -> Retained<Self> {
            // SAFETY: `init` is overridden above to set the ivars first.
            unsafe { msg_send![Self::alloc(), init] }
        }

        /// What this layer reports to.
        pub(crate) fn feedback(&self) -> &Feedback {
            self.ivars()
        }
    }

    /// The feedback of `layer` when it is a [`FluiMetalLayer`].
    pub(crate) fn feedback_of(layer: &AnyObject) -> Option<&Feedback> {
        layer
            .downcast_ref::<FluiMetalLayer>()
            .map(FluiMetalLayer::feedback)
    }

    /// Hand `drawable` a handler that reports to `feedback` when Metal shows
    /// it. Metal calls the handler once, on a thread of its own; the block
    /// holds `feedback` until then.
    fn watch(drawable: &ProtocolObject<dyn CAMetalDrawable>, feedback: Arc<Feedback>) {
        let handler = RcBlock::new(move |drawable: NonNull<ProtocolObject<dyn MTLDrawable>>| {
            os_entry(
                "a drawable's presented handler",
                (),
                AfterPanic::Answer(|()| ()),
                || {
                    // SAFETY: Metal hands the handler the live drawable it
                    // was added to, for the duration of the call.
                    feedback.shown(unsafe { drawable.as_ref() });
                },
            );
        });
        // SAFETY: the block is a valid presented handler; Metal copies it.
        unsafe { drawable.addPresentedHandler(RcBlock::as_ptr(&handler)) };
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, RefreshPeriod};

    use super::{LayerRefresh, NewRefresh, presented};

    /// A drawable's presented time lands as far before the event reading as
    /// it lies before the media clock's reading; a drawable never shown, or a
    /// time that is no number of seconds, reports nothing; the refresh rides
    /// along when known.
    #[test]
    fn a_shown_drawable_is_reported_on_the_event_clock() {
        let refresh = Some(Duration::from_millis(10));
        // Binary fractions, so the seconds convert to nanoseconds exactly.
        let report = presented(500_000_000, 2.5, 2.25, refresh).expect("shown");
        assert_eq!(
            report.at.as_nanos(),
            250_000_000,
            "250 ms before the reading"
        );
        let ahead = presented(500_000_000, 2.25, 2.5, refresh).expect("shown");
        assert_eq!(ahead.at.as_nanos(), 750_000_000, "250 ms after the reading");
        assert_eq!(report.refresh, refresh);
        assert_eq!(
            presented(50_000_000, 100.0, 100.0, None).map(|p| p.refresh),
            Some(None)
        );
        for never in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                presented(50_000_000, 100.0, never, refresh).is_none(),
                "{never} is no shown frame"
            );
        }
    }

    /// The refresh a layer carries follows the display snapshot: pushed when the
    /// window is first placed, again when the same display changes its rate (a
    /// new revision) or the window moves to another display, and not again while
    /// nothing moved.
    #[test]
    fn a_layer_follows_its_display_s_refresh() {
        let registry = DisplayRegistry::new();
        let id = registry
            .mint(&DisplayKey::AndroidId(1))
            .expect("ids are not exhausted");
        let other = registry
            .mint(&DisplayKey::AndroidId(2))
            .expect("ids are not exhausted");
        let at = |hz: f64| {
            let mut display = DisplayInfo::new(
                id,
                "panel",
                Bounds::new(Point::new(0, 0), Size::new(1440, 900)),
                2.0,
            );
            display.refresh_period = RefreshPeriod::from_hz(hz).ok();
            display
        };
        let period = |hz: f64| NewRefresh(RefreshPeriod::from_hz(hz).ok().map(RefreshPeriod::get));
        let first = DisplaySnapshot::empty()
            .successor(vec![at(100.0)])
            .expect("revisions are not exhausted");
        let mut refresh = LayerRefresh::default();
        assert_eq!(refresh.update(&first, Some(id)), Some(period(100.0)));
        assert_eq!(refresh.update(&first, Some(id)), None, "nothing moved");
        let slower = first
            .successor(vec![at(60.0)])
            .expect("revisions are not exhausted");
        assert_eq!(
            refresh.update(&slower, Some(id)),
            Some(period(60.0)),
            "the same display at 60 Hz"
        );
        assert_eq!(
            refresh.update(&slower, Some(other)),
            Some(NewRefresh(None)),
            "a display the snapshot lacks"
        );
        assert_eq!(
            refresh.update(&slower, None),
            Some(NewRefresh(None)),
            "on no display"
        );
    }
}
