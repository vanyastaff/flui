//! The Android displays: the `Display`s the system lists and the Activity's own,
//! read over JNI with the choreographer's refresh rate ([`source`]), and what a
//! sample of a `Display` becomes with no JVM ([`sample`]).
//!
//! `sample` is the pure half, which every host compiles and tests; `source`
//! calls the JVM and the NDK and exists only on Android.

#[cfg(target_os = "android")]
pub(crate) use source::AndroidDisplays;

#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
pub(crate) mod sample {
    //! The rules of the Android display source that need no JVM: what a sample of a
    //! `Display` becomes in the snapshot, its refresh rate as a period (by the
    //! contract's `RefreshPeriod::from_hz`), what rate a window asks for, and
    //! whether the system takes the request at all. Every host runs their tests.

    use std::sync::Arc;

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{
        Availability, CapabilityAbsence, DisplayColor, DisplayId, DisplayInfo, FrameRateHint,
        Headroom, RefreshPeriod,
    };

    use crate::owner::android::ndk::{NdkFn, SetFrameRate, SymbolSource};
    use crate::version::gate::Gate;
    use crate::version::gate::android::AndroidApi;

    /// What one `android.view.Display` reported, before it has an identity.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct RawDisplay {
        /// `Display.getDisplayId`.
        pub(crate) display_id: i32,
        /// `Display.getName`.
        pub(crate) name: String,
        /// The real size in pixels (`getRealMetrics`), in the current rotation.
        pub(crate) width: i32,
        /// See `width`.
        pub(crate) height: i32,
        /// `DisplayMetrics.densityDpi`.
        pub(crate) density_dpi: i32,
        /// The rate of the current mode, in hertz (`Display.getMode`, or
        /// `getRefreshRate` when the mode says nothing).
        pub(crate) refresh_hz: f32,
        /// The highest rate among `getSupportedModes`.
        pub(crate) max_refresh_hz: Option<f32>,
        /// What the display reports of its colour, or why it reports none:
        /// below API 26 there is no such report, and a display whose report
        /// fails loses its colour alone, never the sample.
        pub(crate) color: Result<ColorReport, CapabilityAbsence>,
    }

    /// `Display.isWideColorGamut` and `Display.isHdr` (API 26).
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct DisplayColorReport;

    impl Gate for DisplayColorReport {
        type Version = AndroidApi;
        const SINCE: u64 = 26;
    }

    /// `Display.getHighestHdrSdrRatio` (API 35): how far above SDR white the
    /// display can go, which `getHdrSdrRatio` reports only while HDR content
    /// is on screen.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct HighestHdrSdrRatio;

    impl Gate for HighestHdrSdrRatio {
        type Version = AndroidApi;
        const SINCE: u64 = 35;
    }

    /// What one `Display` reports of its colour.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct ColorReport {
        /// `Display.isWideColorGamut`.
        pub(crate) wide_gamut: bool,
        /// `Display.isHdr`.
        pub(crate) hdr: bool,
        /// `Display.getHighestHdrSdrRatio`; `None` below API 35.
        pub(crate) highest_hdr_sdr_ratio: Option<f32>,
    }

    /// The colour of an Android display from what it reports.
    ///
    /// A display without a report answers why. A wide-gamut display shows
    /// Display P3, Android's wide colour space. An HDR display is high range;
    /// its headroom is the highest HDR/SDR ratio when that is a finite multiple
    /// over white, and absent otherwise (below API 35, or a ratio that is not
    /// one).
    pub(crate) fn display_color(
        report: Result<ColorReport, CapabilityAbsence>,
    ) -> Result<DisplayColor, CapabilityAbsence> {
        let report = report?;
        let gamut = crate::display::source::p3_or_srgb(report.wide_gamut);
        if !report.hdr {
            return Ok(DisplayColor::standard(gamut));
        }
        let headroom = report
            .highest_hdr_sdr_ratio
            .and_then(|ratio| Headroom::new(ratio).ok())
            .filter(|headroom| *headroom > Headroom::SDR)
            .ok_or(CapabilityAbsence::NotOnThisDevice);
        Ok(DisplayColor::high(gamut, headroom))
    }

    impl RawDisplay {
        /// `Display.DEFAULT_DISPLAY`, the built-in screen.
        pub(crate) const DEFAULT_DISPLAY_ID: i32 = 0;

        /// The snapshot entry for this display under `id`; `None` for a display
        /// that reports no size or density, which nothing could be laid out on.
        pub(crate) fn info(&self, id: DisplayId) -> Option<DisplayInfo> {
            if self.width <= 0 || self.height <= 0 || self.density_dpi <= 0 {
                return None;
            }
            let bounds = Bounds::new(Point::new(0, 0), Size::new(self.width, self.height));
            let mut info = DisplayInfo::new(id, &self.name, bounds, scale_factor(self.density_dpi))
                .with_primary(self.display_id == Self::DEFAULT_DISPLAY_ID)
                .with_color(display_color(self.color));
            info.refresh_period = RefreshPeriod::from_hz(f64::from(self.refresh_hz)).ok();
            Some(info)
        }
    }

    /// Device pixels per logical pixel: Android's density is `densityDpi / 160`.
    fn scale_factor(density_dpi: i32) -> f64 {
        f64::from(density_dpi) / 160.0
    }

    /// The highest rate among `rates` that describes a cadence.
    pub(crate) fn max_refresh_hz(rates: impl IntoIterator<Item = f32>) -> Option<f32> {
        rates
            .into_iter()
            .filter(|hz| RefreshPeriod::from_hz(f64::from(*hz)).is_ok())
            .max_by(f32::total_cmp)
    }

    /// The rate to pass to `ANativeWindow_setFrameRate` for `hint`: the display's
    /// highest rate for [`FrameRateHint::High`], `0.0` ("no preference") for the
    /// default. A high rate wanted while the display's highest is not known is
    /// `0.0` too: no rate is guessed, and a high request made earlier, when the rate
    /// was known, is withdrawn rather than left in force.
    pub(crate) fn requested_frame_rate(hint: FrameRateHint, max_hz: Option<f32>) -> f32 {
        match hint {
            FrameRateHint::High => max_hz.unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// The request a window's refresh rate rides on, `ANativeWindow_setFrameRate`,
    /// where this system has it (API 30 and a library that exports it); with it, the
    /// window's `frame_rate_hint` capability.
    pub(crate) struct FrameRateRequests(Option<NdkFn<SetFrameRate>>);

    impl FrameRateRequests {
        /// The request as `library` at `api` has it.
        pub(crate) fn resolve(library: &Arc<dyn SymbolSource>, api: AndroidApi) -> Self {
            Self(NdkFn::resolve(library, api))
        }

        /// The window's `frame_rate_hint` capability: `Native` exactly when the
        /// request resolved.
        pub(crate) const fn availability(&self) -> Availability {
            if self.0.is_some() {
                Availability::Native
            } else {
                Availability::Unavailable
            }
        }

        /// The request, where the system has it.
        #[cfg_attr(
            not(target_os = "android"),
            expect(dead_code, reason = "only the Android source sends the request")
        )]
        pub(crate) const fn handle(&self) -> Option<&NdkFn<SetFrameRate>> {
            self.0.as_ref()
        }
    }

    #[cfg(test)]
    mod frame_rate_tests {
        use std::ffi::{CStr, c_void};
        use std::ptr::NonNull;
        use std::sync::Arc;

        use flui_platform::Availability;

        use super::FrameRateRequests;
        use crate::owner::android::ndk::SymbolSource;
        use crate::version::gate::android::AndroidApi;

        /// A library that exports `ANativeWindow_setFrameRate`, or nothing.
        struct Library(bool);

        impl SymbolSource for Library {
            fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
                // An address no test calls: the lookup only has to find it.
                (self.0 && name == c"ANativeWindow_setFrameRate").then(NonNull::dangling)
            }
        }

        /// A window takes a refresh-rate request exactly where the system has
        /// one: API 30 and a library that exports it.
        #[test]
        fn the_frame_rate_capability_follows_the_request() {
            let cases = [
                (29, true, Availability::Unavailable),
                (30, false, Availability::Unavailable),
                (30, true, Availability::Native),
                (35, true, Availability::Native),
            ];
            for (level, exported, wanted) in cases {
                let library: Arc<dyn SymbolSource> = Arc::new(Library(exported));
                let requests = FrameRateRequests::resolve(&library, AndroidApi::from_level(level));
                assert_eq!(
                    requests.availability(),
                    wanted,
                    "API {level}, exported {exported}"
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{ColorGamut, DisplayKey, DisplayRegistry};

        use super::*;

        fn a_display_without_size_or_density_is_not_described() {
            let registry = DisplayRegistry::new();
            let id = registry.mint(&DisplayKey::AndroidId(0)).expect("id");
            let healthy = RawDisplay {
                display_id: 0,
                name: "Built-in".into(),
                width: 1080,
                height: 2400,
                density_dpi: 420,
                refresh_hz: 120.0,
                max_refresh_hz: Some(120.0),
                color: Err(CapabilityAbsence::NotOnThisDevice),
            };
            let info = healthy.info(id).expect("a healthy display");
            assert_eq!(info.scale_factor, 2.625, "420 dpi is 2.625 logical pixels");
            assert!(info.is_primary, "display 0 is the built-in screen");
            assert_eq!(info.bounds.size, Size::new(1080, 2400));
            assert_eq!(
                info.refresh_period.map(|period| period.get().as_micros()),
                Some(8_333)
            );
            for broken in [
                RawDisplay {
                    width: 0,
                    ..healthy.clone()
                },
                RawDisplay {
                    height: -1,
                    ..healthy.clone()
                },
                RawDisplay {
                    density_dpi: 0,
                    ..healthy.clone()
                },
            ] {
                assert_eq!(broken.info(id), None, "{broken:?}");
            }
            let external = RawDisplay {
                display_id: 2,
                refresh_hz: 0.0,
                ..healthy
            };
            let info = external.info(id).expect("an external display");
            assert!(!info.is_primary);
            assert_eq!(info.refresh_period, None, "an unknown rate stays unknown");
        }

        fn report(wide_gamut: bool, hdr: bool, ratio: Option<f32>) -> ColorReport {
            ColorReport {
                wide_gamut,
                hdr,
                highest_hdr_sdr_ratio: ratio,
            }
        }

        /// A display whose colour report failed is still described: only its
        /// colour is absent, with the reason.
        fn a_display_whose_colour_report_failed_keeps_its_other_facts() {
            let registry = DisplayRegistry::new();
            let id = registry.mint(&DisplayKey::AndroidId(2)).expect("id");
            let failed = RawDisplay {
                display_id: 2,
                name: "External".into(),
                width: 1920,
                height: 1080,
                density_dpi: 160,
                refresh_hz: 60.0,
                max_refresh_hz: Some(60.0),
                color: Err(CapabilityAbsence::NotOnThisDevice),
            };
            let info = failed.info(id).expect("the display is still listed");
            assert_eq!(info.bounds.size, Size::new(1920, 1080));
            assert_eq!(info.color, Err(CapabilityAbsence::NotOnThisDevice));
            let reported = RawDisplay {
                color: Ok(report(true, false, None)),
                ..failed
            };
            assert_eq!(
                reported.info(id).map(|info| info.color),
                Some(Ok(DisplayColor::standard(ColorGamut::DisplayP3)))
            );
        }

        /// The colour reports' gates: none before the API level that has them.
        fn android_colour_gate_table() {
            for (level, color, highest) in [
                (25, false, false),
                (26, true, false),
                (34, true, false),
                (35, true, true),
            ] {
                let api = AndroidApi::from_level(level);
                assert_eq!(
                    api.since::<DisplayColorReport>().is_some(),
                    color,
                    "API {level}"
                );
                assert_eq!(
                    api.since::<HighestHdrSdrRatio>().is_some(),
                    highest,
                    "API {level}"
                );
            }
        }

        /// Each colour report a `Display` gives, to the display colour it means.
        fn android_colour_reports_map_to_the_display_colour() {
            assert_eq!(
                display_color(Err(CapabilityAbsence::NotOnThisDevice)),
                Err(CapabilityAbsence::NotOnThisDevice),
                "below API 26"
            );
            assert_eq!(
                display_color(Ok(report(false, false, None))),
                Ok(DisplayColor::standard(ColorGamut::Srgb))
            );
            assert_eq!(
                display_color(Ok(report(true, false, Some(4.0)))),
                Ok(DisplayColor::standard(ColorGamut::DisplayP3)),
                "a ratio without HDR is no high range"
            );
            assert_eq!(
                display_color(Ok(report(true, true, Some(4.92)))),
                Ok(DisplayColor::high(
                    ColorGamut::DisplayP3,
                    Ok(Headroom::new(4.92).expect("headroom"))
                ))
            );
            for ratio in [
                None,
                Some(1.0),
                Some(f32::NAN),
                Some(f32::INFINITY),
                Some(0.0),
            ] {
                assert_eq!(
                    display_color(Ok(report(true, true, ratio))),
                    Ok(DisplayColor::high(
                        ColorGamut::DisplayP3,
                        Err(CapabilityAbsence::NotOnThisDevice)
                    )),
                    "ratio {ratio:?} publishes no headroom"
                );
            }
        }

        fn highest_rate_ignores_values_that_are_not_rates() {
            assert_eq!(max_refresh_hz([60.0, 120.0, 90.0]), Some(120.0));
            assert_eq!(max_refresh_hz([60.0, f32::NAN, 0.0, 1e9]), Some(60.0));
            assert_eq!(max_refresh_hz([f32::NAN, -1.0]), None);
            assert_eq!(max_refresh_hz([]), None);
        }

        fn the_requested_rate_follows_the_hint() {
            assert_eq!(
                requested_frame_rate(FrameRateHint::High, Some(120.0)),
                120.0
            );
            assert_eq!(
                requested_frame_rate(FrameRateHint::High, None),
                0.0,
                "a highest rate that became unknown withdraws the earlier high request \
                 instead of leaving it in force"
            );
            assert_eq!(
                requested_frame_rate(FrameRateHint::Default, Some(120.0)),
                0.0,
                "the default withdraws the request"
            );
            assert_eq!(requested_frame_rate(FrameRateHint::Default, None), 0.0);
        }

        #[test]
        fn android_display_table() {
            crate::table_test::run_table(
                "android_display_table",
                &[
                    (
                        "a_display_without_size_or_density_is_not_described",
                        a_display_without_size_or_density_is_not_described as fn(),
                    ),
                    (
                        "highest_rate_ignores_values_that_are_not_rates",
                        highest_rate_ignores_values_that_are_not_rates as fn(),
                    ),
                    (
                        "the_requested_rate_follows_the_hint",
                        the_requested_rate_follows_the_hint as fn(),
                    ),
                    (
                        "android_colour_reports_map_to_the_display_colour",
                        android_colour_reports_map_to_the_display_colour as fn(),
                    ),
                    (
                        "android_colour_gate_table",
                        android_colour_gate_table as fn(),
                    ),
                    (
                        "a_display_whose_colour_report_failed_keeps_its_other_facts",
                        a_display_whose_colour_report_failed_keeps_its_other_facts as fn(),
                    ),
                ],
            );
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) mod source {
    //! The Android display source (ADR-0236).
    //!
    //! One JNI sample reads every display (`DisplayManager.getDisplays`): its real
    //! size and density, the rate of its current `Display.Mode`, the highest rate
    //! among its supported modes, and the `Display.getDisplayId` that keys its
    //! identity. The display the Activity is on is read the same way
    //! (`Activity.getDisplay` from API 30, `WindowManager.getDefaultDisplay`
    //! below). `getDisplays()` also lists virtual displays (a cast or a screen
    //! recorder), which appear in the snapshot like any other.
    //!
    //! A display id is never reused within a boot, so a display that is unplugged
    //! and plugged back in comes back under a new id and so a new identity.
    //!
    //! # When the sample is read again
    //!
    //! `display_snapshot()` runs on every owner wake, and a JNI round trip is not
    //! free, so the shared [`DisplaySource`] keeps the last snapshot and the sample
    //! is read again only after something that can change it announced itself:
    //!
    //! - `ConfigChanged` (rotation, density, size) and `Resume` (the Activity may
    //!   have moved to another display while it was away);
    //! - the choreographer's refresh-rate callback, registered on API 30 and later:
    //!   an adaptive-rate switch (60 Hz to 120 Hz) raises no configuration change;
    //! - the Activity's display missing from a sample (it is on a display the
    //!   source has not seen yet): one more sample per snapshot revision.
    //!
    //! Below API 30 the rate is read at the first two events and nowhere else; there
    //! is no polling. The callback runs in the application's own `poll_events` and
    //! only invalidates the source and wakes the owner, which does the read.

    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread::ThreadId;

    use android_activity::AndroidApp;
    use android_activity::ndk::native_window::NativeWindow;
    use flui_platform::{CapabilityAbsence, DisplayId, DisplayKey, DisplaySnapshot, FrameRateHint};
    use jni::{
        Env, JValue, jni_sig, jni_str,
        objects::{JObject, JObjectArray, JString},
    };
    use parking_lot::Mutex;

    use super::sample::{
        ColorReport, DisplayColorReport, FrameRateRequests, HighestHdrSdrRatio, RawDisplay,
        max_refresh_hz, requested_frame_rate,
    };
    use crate::display::source::DisplaySource;
    use crate::error::PlatformError;
    use crate::owner::android::ndk::{AChoreographer, RefreshRateWatch, SymbolSource};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::owner::signal::OwnerSignal;
    use crate::owner::watch::Watch;
    use crate::version::gate::android::{AndroidApi, BackgroundBlur, ContextDisplay};
    use flui_platform::PreferenceQueryError;

    /// `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`.
    const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;

    /// What the last sample said about the display the Activity is on.
    #[derive(Clone, Copy)]
    struct ActivityDisplay {
        id: DisplayId,
        max_hz: Option<f32>,
    }

    /// Marks the source stale and asks the owner for a turn to read it. Shared with
    /// the choreographer callback, which holds it through a raw `Arc` pointer.
    struct Invalidator {
        source: Arc<DisplaySource>,
        owner: Weak<OwnerSignal>,
    }

    impl Invalidator {
        fn raise(&self) {
            self.source.invalidate();
            self.wake();
        }

        fn wake(&self) {
            if let Some(owner) = self.owner.upgrade() {
                // A failed wake leaves the owner's own retry debt.
                let _ = owner.wake();
            }
        }
    }

    /// The state of one registered refresh-rate callback.
    ///
    /// Dropping it takes the callback down *before* the data it points at is freed,
    /// so a late call cannot reach freed memory. It is dropped on the thread that
    /// registered it, whose looper delivers the callback, when the loop drops the
    /// watch `observe` returned.
    struct Registration {
        watch: RefreshRateWatch,
        choreographer: NonNull<AChoreographer>,
        /// `Arc::into_raw` of the invalidator the callback raises.
        data: *const Invalidator,
        thread: ThreadId,
    }

    impl Drop for Registration {
        #[expect(
            unsafe_code,
            reason = "unregisters the choreographer callback, then frees its data"
        )]
        fn drop(&mut self) {
            if std::thread::current().id() != self.thread {
                // The callback may be running on the looper thread right now; only
                // that thread can take it down safely. Leaking the data keeps a
                // late call valid.
                tracing::warn!("refresh-rate callback outlived its thread; leaking its data");
                return;
            }
            // SAFETY: `choreographer` is this thread's instance, which lives as long
            // as the thread; the callback and data are the pair registered in
            // `observe`, and `unregister` is the NDK function of that name.
            unsafe {
                (self.watch.unregister.get())(
                    self.choreographer.as_ptr(),
                    on_refresh_rate,
                    self.data.cast_mut().cast(),
                );
            }
            // SAFETY: `data` came from `Arc::into_raw` and is released once, after
            // the callback was unregistered, so nothing can reach it any more.
            drop(unsafe { Arc::from_raw(self.data) });
        }
    }

    /// The choreographer's refresh-rate callback: the display's rate changed.
    ///
    /// Runs on the thread that registered it, from its looper.
    #[expect(
        unsafe_code,
        reason = "an extern \"C\" callback receiving the registered data"
    )]
    unsafe extern "C" fn on_refresh_rate(period_nanos: i64, data: *mut c_void) {
        os_entry(
            "the choreographer's refresh-rate callback",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                // SAFETY: `data` is the `Arc<Invalidator>` pointer registered with
                // this callback, which stays allocated until the callback is
                // unregistered.
                let invalidator = unsafe { &*data.cast::<Invalidator>() };
                tracing::debug!(period_nanos, "Android: the display refresh rate changed");
                invalidator.raise();
            },
        );
    }

    /// The displays of the device, sampled through JNI and kept between samples.
    pub struct AndroidDisplays {
        source: Arc<DisplaySource>,
        api: AndroidApi,
        library: Arc<dyn SymbolSource>,
        frame_rate: FrameRateRequests,
        invalidator: Arc<Invalidator>,
        activity: Mutex<Option<ActivityDisplay>>,
        /// Set when a sample found another highest rate for the Activity's display,
        /// until the window has asked again with it.
        max_rate_changed: AtomicBool,
        /// `WindowManager.isCrossWindowBlurEnabled` at the last sample; false
        /// below API 31 and before the first sample.
        cross_window_blur: AtomicBool,
    }

    impl std::fmt::Debug for AndroidDisplays {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AndroidDisplays").finish_non_exhaustive()
        }
    }

    impl AndroidDisplays {
        pub(crate) fn new(
            api: AndroidApi,
            library: Arc<dyn SymbolSource>,
            owner: Weak<OwnerSignal>,
        ) -> Self {
            let source = Arc::new(DisplaySource::new());
            Self {
                invalidator: Arc::new(Invalidator {
                    source: Arc::clone(&source),
                    owner,
                }),
                source,
                api,
                frame_rate: FrameRateRequests::resolve(&library, api),
                library,
                activity: Mutex::new(None),
                max_rate_changed: AtomicBool::new(false),
                cross_window_blur: AtomicBool::new(false),
            }
        }
    }

    impl crate::owner::android::HostDisplays for AndroidDisplays {
        /// The display state may have changed (configuration change, resume): read
        /// it again at the next owner turn, which this asks for.
        fn invalidate(&self) {
            self.invalidator.raise();
        }

        /// The snapshot: the kept one while nothing announced a change, a fresh
        /// sample otherwise.
        fn snapshot(&self, app: &AndroidApp) -> Result<DisplaySnapshot, PlatformError> {
            let before = self.source.published().revision();
            let mut missing_activity = None;
            let snapshot = self.source.snapshot_with(|| {
                let sample = sample(app, self.api).map_err(|error| PlatformError::Displays {
                    message: error.to_string(),
                })?;
                let mut infos = Vec::with_capacity(sample.displays.len());
                let mut activity = None;
                for raw in &sample.displays {
                    let Some(id) = self.source.mint(&DisplayKey::AndroidId(raw.display_id)) else {
                        continue;
                    };
                    let Some(info) = raw.info(id) else {
                        continue;
                    };
                    if raw.display_id == sample.activity_display_id {
                        activity = Some(ActivityDisplay {
                            id,
                            max_hz: raw.max_refresh_hz,
                        });
                    }
                    infos.push(info);
                }
                if activity.is_none() {
                    missing_activity = Some(sample.activity_display_id);
                }
                self.cross_window_blur
                    .store(sample.cross_window_blur, Ordering::Release);
                let previous = std::mem::replace(&mut *self.activity.lock(), activity);
                if previous.and_then(|display| display.max_hz)
                    != activity.and_then(|display| display.max_hz)
                {
                    self.max_rate_changed.store(true, Ordering::Release);
                }
                Ok(infos)
            })?;
            if let Some(display_id) = missing_activity {
                // The Activity is on a display this sample does not list: ask for
                // one more sample per snapshot revision, not one per owner wake.
                // An idle app has no other reason to wake, so the first miss asks
                // the owner for the turn that samples again.
                self.source
                    .note_miss_with(u64::from(display_id.unsigned_abs()), || {
                        self.invalidator.wake();
                    });
            }
            if snapshot.revision() != before {
                log_snapshot(&snapshot, *self.activity.lock(), before.get() == 0);
            }
            Ok(snapshot)
        }

        /// The display the Activity was on at the last sample.
        fn activity_display(&self) -> Option<DisplayId> {
            self.activity.lock().map(|activity| activity.id)
        }

        /// Whether the highest rate of the Activity's display changed since this was
        /// last asked; reading clears it. A hint the window skipped for want of the
        /// rate is then asked again.
        fn take_max_rate_change(&self) -> bool {
            self.max_rate_changed.swap(false, Ordering::AcqRel)
        }

        fn frame_rate_hint(&self) -> flui_platform::Availability {
            self.frame_rate.availability()
        }

        fn cross_window_blur(&self) -> bool {
            self.cross_window_blur.load(Ordering::Acquire)
        }

        /// Ask the system for the refresh rate `hint` names on `window`, if this
        /// system has the request (API 30).
        #[expect(unsafe_code, reason = "calls ANativeWindow_setFrameRate")]
        fn request_frame_rate(&self, window: &NativeWindow, hint: FrameRateHint) {
            let Some(set_frame_rate) = self.frame_rate.handle() else {
                return;
            };
            let max_hz = self.activity.lock().and_then(|activity| activity.max_hz);
            let rate = requested_frame_rate(hint, max_hz);
            // SAFETY: `window` is a live reference to an `ANativeWindow` for the
            // duration of this call, and the function takes the `ANativeWindow*`, a
            // rate and a compatibility value.
            let status = unsafe {
                (set_frame_rate.get())(
                    window.ptr().as_ptr(),
                    rate,
                    FRAME_RATE_COMPATIBILITY_DEFAULT,
                )
            };
            if status == 0 {
                tracing::debug!(
                    rate,
                    high = matches!(hint, FrameRateHint::High),
                    "Android: frame rate requested"
                );
            } else {
                tracing::warn!(
                    status,
                    rate,
                    "Android: ANativeWindow_setFrameRate was refused"
                );
            }
        }

        /// Register for the choreographer's refresh-rate callback, on API 30 and
        /// later, on the calling thread, whose looper delivers it. The watch takes
        /// the callback down when dropped, on the thread that registered it; an
        /// unwind out of the loop drops it as well.
        #[expect(unsafe_code, reason = "registers a choreographer callback")]
        fn observe(&self) -> Watch {
            let Some(functions) = RefreshRateWatch::resolve(&self.library, self.api) else {
                tracing::debug!(
                    api = %self.api,
                    "Android: no refresh-rate callback; the rate is read on configuration changes and resume"
                );
                return Watch::new(());
            };
            // SAFETY: `AChoreographer_getInstance` takes nothing and returns this
            // thread's instance, or null without a looper.
            let choreographer = unsafe { (functions.instance.get())() };
            let Some(choreographer) = NonNull::new(choreographer) else {
                tracing::warn!(
                    "Android: no choreographer on the app thread; refresh rate changes are not watched"
                );
                return Watch::new(());
            };
            let data = Arc::into_raw(Arc::clone(&self.invalidator));
            // SAFETY: the callback and data are the pair `Registration::drop`
            // unregisters before freeing `data`; the choreographer is this
            // thread's.
            unsafe {
                (functions.register.get())(
                    choreographer.as_ptr(),
                    on_refresh_rate,
                    data.cast_mut().cast(),
                );
            }
            Watch::new(Registration {
                watch: functions,
                choreographer,
                data,
                thread: std::thread::current().id(),
            })
        }
    }

    /// One line per display each time the snapshot changes, which is rare: the way
    /// to see on a device what the source read. The first, at startup, is `info`;
    /// later ones are `debug`.
    fn log_snapshot(snapshot: &DisplaySnapshot, activity: Option<ActivityDisplay>, first: bool) {
        macro_rules! display_line {
            ($level:expr, $info:expr, $on_activity:expr, $refresh_period_us:expr) => {
                tracing::event!(
                    $level,
                    revision = snapshot.revision().get(),
                    id = $info.id.get(),
                    name.public = %$info.name,
                    width = $info.bounds.size.width,
                    height = $info.bounds.size.height,
                    scale_factor = $info.scale_factor,
                    refresh_period_us = $refresh_period_us,
                    primary = $info.is_primary,
                    color.public = ?$info.color,
                    on_activity = $on_activity,
                    "Android display"
                )
            };
        }
        for info in snapshot.displays() {
            let refresh_period_us = info.refresh_period.map_or(0, |period| {
                u64::try_from(period.get().as_micros()).unwrap_or(u64::MAX)
            });
            let on_activity = activity.is_some_and(|activity| activity.id == info.id);
            if first {
                display_line!(tracing::Level::INFO, info, on_activity, refresh_period_us);
            } else {
                display_line!(tracing::Level::DEBUG, info, on_activity, refresh_period_us);
            }
        }
    }

    /// What one JNI read found.
    struct Sample {
        displays: Vec<RawDisplay>,
        /// The display the Activity is on.
        activity_display_id: i32,
        /// Whether the device blurs behind windows now; false below API 31.
        cross_window_blur: bool,
    }

    fn sample(app: &AndroidApp, api: AndroidApi) -> Result<Sample, PreferenceQueryError> {
        crate::owner::android::activity::with_activity(app, |env, activity| {
            env.with_local_frame(64, |env| -> jni::errors::Result<Sample> {
                let activity_display_id = activity_display_id(env, activity, api)?;
                let service = env
                    .get_static_field(
                        jni_str!("android/content/Context"),
                        jni_str!("DISPLAY_SERVICE"),
                        jni_sig!("Ljava/lang/String;"),
                    )?
                    .l()?;
                let manager = env
                    .call_method(
                        activity,
                        jni_str!("getSystemService"),
                        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                        &[JValue::Object(&service)],
                    )?
                    .l()?;
                let listed = env
                    .call_method(
                        &manager,
                        jni_str!("getDisplays"),
                        jni_sig!("()[Landroid/view/Display;"),
                        &[],
                    )?
                    .l()?;
                let listed = JObjectArray::<JObject<'_>>::cast_local(env, listed)?;
                let count = listed.len(env)?;
                let mut displays = Vec::with_capacity(count);
                for index in 0..count {
                    displays.push(env.with_local_frame(
                        32,
                        |env| -> jni::errors::Result<RawDisplay> {
                            let display = listed.get_element(env, index)?;
                            read_display(env, &display, api)
                        },
                    )?);
                }
                let cross_window_blur = if api.since::<BackgroundBlur>().is_some() {
                    let windows = env
                        .call_method(
                            activity,
                            jni_str!("getWindowManager"),
                            jni_sig!("()Landroid/view/WindowManager;"),
                            &[],
                        )?
                        .l()?;
                    env.call_method(
                        &windows,
                        jni_str!("isCrossWindowBlurEnabled"),
                        jni_sig!("()Z"),
                        &[],
                    )?
                    .z()?
                } else {
                    false
                };
                Ok(Sample {
                    displays,
                    activity_display_id,
                    cross_window_blur,
                })
            })
        })
    }

    /// `Display.getDisplayId` of the display the Activity is on.
    fn activity_display_id(
        env: &mut Env<'_>,
        activity: &JObject<'_>,
        api: AndroidApi,
    ) -> jni::errors::Result<i32> {
        if api.since::<ContextDisplay>().is_some() {
            match env.call_method(
                activity,
                jni_str!("getDisplay"),
                jni_sig!("()Landroid/view/Display;"),
                &[],
            ) {
                Ok(display) => {
                    let display = display.l()?;
                    if !display.is_null() {
                        return env
                            .call_method(&display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
                            .i();
                    }
                }
                // A context that is not tied to a display refuses; use the window manager.
                Err(_) => env.exception_clear(),
            }
        }
        let windows = env
            .call_method(
                activity,
                jni_str!("getWindowManager"),
                jni_sig!("()Landroid/view/WindowManager;"),
                &[],
            )?
            .l()?;
        let display = env
            .call_method(
                &windows,
                jni_str!("getDefaultDisplay"),
                jni_sig!("()Landroid/view/Display;"),
                &[],
            )?
            .l()?;
        env.call_method(&display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
            .i()
    }

    fn read_display(
        env: &mut Env<'_>,
        display: &JObject<'_>,
        api: AndroidApi,
    ) -> jni::errors::Result<RawDisplay> {
        let display_id = env
            .call_method(display, jni_str!("getDisplayId"), jni_sig!("()I"), &[])?
            .i()?;
        let name = env
            .call_method(
                display,
                jni_str!("getName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let name = if name.is_null() {
            String::new()
        } else {
            env.cast_local::<JString<'_>>(name)?.try_to_string(env)?
        };

        let metrics = env.new_object(
            jni_str!("android/util/DisplayMetrics"),
            jni_sig!("()V"),
            &[],
        )?;
        env.call_method(
            display,
            jni_str!("getRealMetrics"),
            jni_sig!("(Landroid/util/DisplayMetrics;)V"),
            &[JValue::Object(&metrics)],
        )?;
        let width = env
            .get_field(&metrics, jni_str!("widthPixels"), jni_sig!("I"))?
            .i()?;
        let height = env
            .get_field(&metrics, jni_str!("heightPixels"), jni_sig!("I"))?
            .i()?;
        let density_dpi = env
            .get_field(&metrics, jni_str!("densityDpi"), jni_sig!("I"))?
            .i()?;

        // The rate of the mode the display runs now; `getRefreshRate` answers the
        // same on every system that has modes and stands in when the mode is absent.
        let mode = env
            .call_method(
                display,
                jni_str!("getMode"),
                jni_sig!("()Landroid/view/Display$Mode;"),
                &[],
            )?
            .l()?;
        let mut refresh_hz = if mode.is_null() {
            0.0
        } else {
            env.call_method(&mode, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
                .f()?
        };
        if refresh_hz <= 0.0 {
            refresh_hz = env
                .call_method(display, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
                .f()?;
        }

        let modes = env
            .call_method(
                display,
                jni_str!("getSupportedModes"),
                jni_sig!("()[Landroid/view/Display$Mode;"),
                &[],
            )?
            .l()?;
        let modes = JObjectArray::<JObject<'_>>::cast_local(env, modes)?;
        let mut rates = Vec::new();
        for index in 0..modes.len(env)? {
            let rate = env.with_local_frame(8, |env| -> jni::errors::Result<f32> {
                let mode = modes.get_element(env, index)?;
                env.call_method(&mode, jni_str!("getRefreshRate"), jni_sig!("()F"), &[])?
                    .f()
            })?;
            rates.push(rate);
        }

        Ok(RawDisplay {
            display_id,
            name,
            width,
            height,
            density_dpi,
            refresh_hz,
            max_refresh_hz: max_refresh_hz(rates),
            color: read_color(env, display, api),
        })
    }

    /// What `display` reports of its colour, on the APIs that have each report.
    /// What `display` reports of its colour, on the APIs that have each report.
    ///
    /// A report that fails (an exception from the display, a JNI error) is
    /// this display's colour gone, not the sample: the exception is cleared
    /// and the display answers `NotOnThisDevice`, while every other display
    /// and this one's geometry still reach the snapshot.
    fn read_color(
        env: &mut Env<'_>,
        display: &JObject<'_>,
        api: AndroidApi,
    ) -> Result<ColorReport, CapabilityAbsence> {
        if api.since::<DisplayColorReport>().is_none() {
            return Err(CapabilityAbsence::NotOnThisDevice);
        }
        query_color(env, display, api).map_err(|error| {
            env.exception_clear();
            tracing::warn!(%error, "Android: a display's colour report failed");
            CapabilityAbsence::NotOnThisDevice
        })
    }

    fn query_color(
        env: &mut Env<'_>,
        display: &JObject<'_>,
        api: AndroidApi,
    ) -> jni::errors::Result<ColorReport> {
        let wide_gamut = env
            .call_method(display, jni_str!("isWideColorGamut"), jni_sig!("()Z"), &[])?
            .z()?;
        let hdr = env
            .call_method(display, jni_str!("isHdr"), jni_sig!("()Z"), &[])?
            .z()?;
        let highest_hdr_sdr_ratio = if api.since::<HighestHdrSdrRatio>().is_some() {
            Some(
                env.call_method(
                    display,
                    jni_str!("getHighestHdrSdrRatio"),
                    jni_sig!("()F"),
                    &[],
                )?
                .f()?,
            )
        } else {
            None
        };
        Ok(ColorReport {
            wide_gamut,
            hdr,
            highest_hdr_sdr_ratio,
        })
    }
}
