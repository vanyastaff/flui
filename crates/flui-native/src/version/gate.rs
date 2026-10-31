//! The running OS version of each environment, and the gates above its floor (ADR-0244).
//!
//! Each environment has one version type, read once when its platform is built and kept there
//! (never in a `static`): `windows::WindowsBuild`, `apple::AppleVersion`, `android::AndroidApi`.
//! A doc build compiles only its target's module, so they are named here, not linked. The
//! number inside is private to the type, so no code outside this module compares a version with
//! a literal. Each environment's module is built for its target and for tests, which check its
//! rules on every host; reading the running version is the one part that needs the system.
//!
//! A feature newer than the floor is a marker type implementing [`Gate`], declared next to the
//! code that uses it, with a doc line saying what that code does without it. The version's
//! `since::<F>()` answers with a [`Since<F>`] proof or nothing, and the call that needs the
//! feature takes the proof. A gate at or below the floor does not compile: the API is then called
//! directly, and a floor rise turns the gates it makes dead into compile errors. A version that
//! could not be read unlocks nothing.

use std::marker::PhantomData;

use flui_platform::{Environment, OsRelease};

use crate::PlatformError;

/// An environment's version type: its floor, and an order over its values.
pub(crate) trait OsVersion: Copy + std::fmt::Display {
    /// The environment a refusal below the floor names.
    const ENVIRONMENT: Environment;
    /// The floor itself, as a refusal shows it.
    const FLOOR_RELEASE: Self;
    /// The floor's position in [`ordinal`](Self::ordinal)'s order.
    const FLOOR: u64;
    /// The application's own floor, when `flui build` raised FLUI's from `flui.toml`
    /// (ADR-0244 §10); a refusal compares against the higher of the two.
    const APP_FLOOR: Option<Self>;

    /// The version's position in an order every version of this type agrees with; `None` when
    /// the version could not be read.
    fn ordinal(self) -> Option<u64>;
}

/// [`PlatformError::UnsupportedOs`] when `version` is below its floor (ADR-0244 §3): FLUI's, or
/// the application's when it is higher (§10). A version that could not be read is not below
/// it: the platform starts, and no gate opens.
pub(crate) fn refuse_below_floor<V: OsVersion>(version: V) -> Result<(), PlatformError> {
    refuse_below(version, V::APP_FLOOR)
}

/// [`refuse_below_floor`] with the application's floor `app` given.
fn refuse_below<V: OsVersion>(version: V, app: Option<V>) -> Result<(), PlatformError> {
    let (floor, shown) = match app.and_then(|app| Some((app.ordinal()?, app))) {
        Some((ordinal, app)) if ordinal > V::FLOOR => (ordinal, app),
        _ => (V::FLOOR, V::FLOOR_RELEASE),
    };
    match version.ordinal() {
        Some(ordinal) if ordinal < floor => Err(PlatformError::UnsupportedOs {
            environment: V::ENVIRONMENT,
            found: OsRelease::from_display(version),
            floor: OsRelease::from_display(shown),
        }),
        Some(_) | None => Ok(()),
    }
}

/// A feature that exists from [`SINCE`](Self::SINCE) on, in the versions of
/// [`Version`](Self::Version).
pub(crate) trait Gate {
    /// The version type the feature is gated on.
    type Version: OsVersion;
    /// The first version that has the feature, as an [`OsVersion::ordinal`].
    const SINCE: u64;
}

/// Zero-sized evidence that the running version has feature `F`. Only [`since`] makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Since<F>(PhantomData<fn() -> F>);

/// The proof that `version` has feature `F`, or `None` when it predates `F` or is unknown.
///
/// Fails to compile when `F` is not above the floor, where the feature is always there.
pub(crate) fn since<F: Gate>(version: F::Version) -> Option<Since<F>> {
    const {
        assert!(
            F::SINCE > <F::Version as OsVersion>::FLOOR,
            "a gate at or below the floor: call the API directly"
        );
    }
    version
        .ordinal()
        .filter(|ordinal| *ordinal >= F::SINCE)
        .map(|_| Since(PhantomData))
}

/// The Windows build: its order and its floor. The build the process runs on is read by
/// `read_build`, the one part that calls the system.
#[cfg(any(windows, test))]
#[forbid(unsafe_code)]
pub(crate) mod windows {
    use crate::version::floor::WINDOWS_BUILD;

    use flui_platform::Environment;

    use super::{Gate, OsVersion, Since};

    /// The OS build number (`dwBuildNumber`); private (see the module above).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct WindowsBuild(u32);

    impl WindowsBuild {
        /// A build the system could not report, which unlocks nothing.
        pub(crate) const UNKNOWN: Self = Self(0);

        /// The build the system reported.
        pub(crate) const fn from_number(build: u32) -> Self {
            Self(build)
        }

        /// The proof that this build has feature `F`, or `None`.
        pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
            super::since::<F>(self)
        }
    }

    impl OsVersion for WindowsBuild {
        const ENVIRONMENT: Environment = Environment::Windows;
        const FLOOR_RELEASE: Self = Self(WINDOWS_BUILD);
        const FLOOR: u64 = WINDOWS_BUILD as u64;
        const APP_FLOOR: Option<Self> = match crate::version::app_floor::WINDOWS {
            Some(build) => Some(Self(build)),
            None => None,
        };

        fn ordinal(self) -> Option<u64> {
            (self != Self::UNKNOWN).then_some(u64::from(self.0))
        }
    }

    /// Shown as `build N`, for logs and about screens; carries no order.
    impl std::fmt::Display for WindowsBuild {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "build {}", self.0)
        }
    }
}

/// Reading the build the process runs on, with `RtlGetVersion`.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "reads the build with RtlGetVersion; the block names its contract"
)]
mod read_build {
    use windows::{
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
    };

    use super::windows::WindowsBuild;

    impl WindowsBuild {
        /// Read the running build with `RtlGetVersion`, which, unlike `GetVersionExW`, does not
        /// report the manifest's compatibility version. A failed read is
        /// [`UNKNOWN`](Self::UNKNOWN).
        pub(crate) fn read() -> Self {
            let mut info = OSVERSIONINFOW {
                dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
                ..Default::default()
            };
            // SAFETY: `info` is an `OSVERSIONINFOW` whose size field holds its own
            // size, which is what `RtlGetVersion` requires; it outlives the call.
            let status = unsafe { RtlGetVersion(&raw mut info) };
            if status.is_err() {
                tracing::warn!(
                    ?status,
                    "the Windows build is unknown; gated flags stay off"
                );
                return Self::UNKNOWN;
            }
            Self::from_number(info.dwBuildNumber)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::OsVersion;
        use super::WindowsBuild;

        #[test]
        fn the_running_build_is_read() {
            assert!(
                WindowsBuild::read()
                    .ordinal()
                    .is_some_and(|build| build >= 10_000),
                "RtlGetVersion reports the real build, not a manifest-compatible one"
            );
        }
    }
}

/// The Apple operating-system versions: iOS's ([`AppleVersion`]) and macOS's
/// ([`MacOSVersion`]), the same three components over separate floors.
#[cfg(any(target_os = "ios", target_os = "macos", test))]
#[forbid(unsafe_code)]
pub(crate) mod apple {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    use objc2_foundation::NSProcessInfo;

    #[cfg(any(target_os = "ios", test))]
    use crate::version::floor::IOS_FLOOR;
    #[cfg(any(target_os = "macos", test))]
    use crate::version::floor::MACOS_FLOOR;

    use flui_platform::Environment;

    use super::{Gate, OsVersion, Since};

    /// An operating-system version as `NSProcessInfo.operatingSystemVersion` reports it. Ordered
    /// by major, then minor, then patch; the components are private (see the module above).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct AppleVersion {
        major: u32,
        minor: u32,
        patch: u32,
    }

    impl AppleVersion {
        /// A version Foundation could not report, which unlocks nothing.
        pub(crate) const UNKNOWN: Self = Self::new(0, 0, 0);

        pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
            Self {
                major,
                minor,
                patch,
            }
        }

        /// The version the process runs on: `NSProcessInfo.operatingSystemVersion`. The
        /// platform reads it when it is built and keeps the value; a feature newer than the
        /// deployment target is then gated on a proof from [`since`](Self::since).
        #[cfg(any(target_os = "ios", target_os = "macos"))]
        pub(crate) fn read() -> Self {
            let version = NSProcessInfo::processInfo().operatingSystemVersion();
            let component = |value: isize| i64::try_from(value).unwrap_or(0);
            Self::from_components(
                component(version.majorVersion),
                component(version.minorVersion),
                component(version.patchVersion),
            )
        }

        /// The version from the signed components Foundation reports; a component that is
        /// negative or too large for `u32` reads as zero.
        pub(crate) fn from_components(major: i64, minor: i64, patch: i64) -> Self {
            let component = |value: i64| u32::try_from(value).unwrap_or(0);
            Self::new(component(major), component(minor), component(patch))
        }

        /// The version's position in the order of [`OsVersion::ordinal`], for a
        /// [`Gate::SINCE`].
        ///
        /// Each component keeps 21 bits; a component past that saturates, which no Apple release
        /// reaches.
        pub(crate) const fn rank(self) -> u64 {
            const fn component(value: u32) -> u64 {
                const MAX: u64 = (1 << 21) - 1;
                let value = value as u64;
                if value > MAX { MAX } else { value }
            }
            (component(self.major) << 42) | (component(self.minor) << 21) | component(self.patch)
        }

        /// The proof that this iOS version has feature `F`, or `None`.
        #[cfg(any(target_os = "ios", test))]
        pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
            super::since::<F>(self)
        }
    }

    /// As an iOS version, over iOS's floor.
    #[cfg(any(target_os = "ios", test))]
    impl OsVersion for AppleVersion {
        const ENVIRONMENT: Environment = Environment::iOS;
        const FLOOR_RELEASE: Self = AppleVersion::new(IOS_FLOOR.0, IOS_FLOOR.1, IOS_FLOOR.2);
        const FLOOR: u64 = Self::FLOOR_RELEASE.rank();
        const APP_FLOOR: Option<Self> = match crate::version::app_floor::IOS {
            Some((major, minor, patch)) => Some(AppleVersion::new(major, minor, patch)),
            None => None,
        };

        fn ordinal(self) -> Option<u64> {
            (self != Self::UNKNOWN).then(|| self.rank())
        }
    }

    /// A macOS version: [`AppleVersion`]'s components over macOS's floor, so a
    /// gate written for one system cannot be asked of the other.
    #[cfg(any(target_os = "macos", test))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct MacOSVersion(AppleVersion);

    #[cfg(any(target_os = "macos", test))]
    impl MacOSVersion {
        /// A version Foundation could not report, which unlocks nothing.
        #[cfg(test)]
        pub(crate) const UNKNOWN: Self = Self(AppleVersion::UNKNOWN);

        pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
            Self(AppleVersion::new(major, minor, patch))
        }

        /// The version the process runs on, which the platform reads once and
        /// keeps.
        #[cfg(target_os = "macos")]
        pub(crate) fn read() -> Self {
            Self(AppleVersion::read())
        }

        /// The version's position in the order of [`OsVersion::ordinal`], for a
        /// [`Gate::SINCE`].
        pub(crate) const fn rank(self) -> u64 {
            self.0.rank()
        }

        /// The proof that this macOS version has feature `F`, or `None`.
        pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
            super::since::<F>(self)
        }
    }

    #[cfg(any(target_os = "macos", test))]
    impl OsVersion for MacOSVersion {
        const ENVIRONMENT: Environment = Environment::MacOS;
        const FLOOR_RELEASE: Self = MacOSVersion::new(MACOS_FLOOR.0, MACOS_FLOOR.1, MACOS_FLOOR.2);
        const FLOOR: u64 = Self::FLOOR_RELEASE.rank();
        const APP_FLOOR: Option<Self> = match crate::version::app_floor::MACOS {
            Some((major, minor, patch)) => Some(MacOSVersion::new(major, minor, patch)),
            None => None,
        };

        fn ordinal(self) -> Option<u64> {
            (self.0 != AppleVersion::UNKNOWN).then(|| self.rank())
        }
    }

    /// Shown as `major.minor.patch`, for logs; carries no order.
    #[cfg(any(target_os = "macos", test))]
    impl std::fmt::Display for MacOSVersion {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(formatter)
        }
    }

    /// Shown as `major.minor.patch`, for logs and about screens; carries no order.
    impl std::fmt::Display for AppleVersion {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{AppleVersion, Gate};

        struct Feature;

        impl Gate for Feature {
            type Version = AppleVersion;
            const SINCE: u64 = AppleVersion::new(15, 2, 0).rank();
        }

        #[test]
        fn a_version_proves_a_feature_from_its_first_release_on() {
            let cases = [
                ((14, 8, 9), false),
                ((15, 1, 99), false),
                ((15, 2, 0), true),
                ((15, 2, 1), true),
                ((16, 0, 0), true),
                ((0, 0, 0), false),
            ];
            for ((major, minor, patch), has) in cases {
                let proof = AppleVersion::new(major, minor, patch).since::<Feature>();
                assert_eq!(proof.is_some(), has, "{major}.{minor}.{patch}");
            }
        }

        #[test]
        fn components_outside_u32_read_as_zero() {
            assert_eq!(
                AppleVersion::from_components(17, 4, 1),
                AppleVersion::new(17, 4, 1)
            );
            assert_eq!(
                AppleVersion::from_components(-1, 4, 1),
                AppleVersion::new(0, 4, 1)
            );
            assert_eq!(
                AppleVersion::from_components(17, i64::MAX, 1),
                AppleVersion::new(17, 0, 1)
            );
        }
    }
}

/// The Android API level, and the framework APIs above the floor the backend uses.
///
/// The level is read in [`ndk`](crate::owner::android::ndk), where the NDK functions it comes
/// from are looked up; an NDK function above the floor is a gate of its own there.
#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
pub(crate) mod android {
    use crate::version::floor::ANDROID_API;

    use flui_platform::Environment;

    use super::{Gate, OsVersion, Since};

    /// The API level of the running system (`Build.VERSION.SDK_INT`); private (see the module
    /// above).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct AndroidApi(u32);

    impl AndroidApi {
        /// A level the system would not report, which unlocks nothing.
        pub(crate) const UNKNOWN: Self = Self(0);

        /// The level the system reported.
        pub(crate) const fn from_level(level: u32) -> Self {
            Self(level)
        }

        /// The proof that this level has feature `F`, or `None`.
        pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
            super::since::<F>(self)
        }
    }

    impl OsVersion for AndroidApi {
        const ENVIRONMENT: Environment = Environment::Android;
        const FLOOR_RELEASE: Self = Self(ANDROID_API);
        const FLOOR: u64 = ANDROID_API as u64;
        const APP_FLOOR: Option<Self> = match crate::version::app_floor::ANDROID {
            Some(level) => Some(Self(level)),
            None => None,
        };

        fn ordinal(self) -> Option<u64> {
            (self != Self::UNKNOWN).then_some(u64::from(self.0))
        }
    }

    /// Shown as `API N`, for logs and about screens; carries no order.
    impl std::fmt::Display for AndroidApi {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if *self == Self::UNKNOWN {
                formatter.write_str("API unknown")
            } else {
                write!(formatter, "API {}", self.0)
            }
        }
    }

    /// `Context.getDisplay` (API 30). Without it the display comes from the window manager's
    /// default display.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ContextDisplay;

    impl Gate for ContextDisplay {
        type Version = AndroidApi;
        const SINCE: u64 = 30;
    }

    /// `Configuration.fontWeightAdjustment` (API 31). Without it the text-weight preference is
    /// reported as unavailable.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct TextWeight;

    impl Gate for TextWeight {
        type Version = AndroidApi;
        const SINCE: u64 = 31;
    }

    /// `ViewConfiguration.getScaledHorizontalScrollFactor` and its vertical twin (API 26). Without
    /// them the scroll factor is read from the theme's `listPreferredItemHeight`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ScaledScrollFactor;

    impl Gate for ScaledScrollFactor {
        type Version = AndroidApi;
        const SINCE: u64 = 26;
    }

    /// `WindowInsets.Type` and `WindowInsets.getInsetsIgnoringVisibility` (API 30). Without them
    /// the safe area is the stable insets, with the display cutout merged in where
    /// [`DisplayCutout`] exists.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct InsetsByType;

    impl Gate for InsetsByType {
        type Version = AndroidApi;
        const SINCE: u64 = 30;
    }

    /// `WindowInsets.getDisplayCutout` (API 28). Without it the safe area leaves a cutout out.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct DisplayCutout;

    impl Gate for DisplayCutout {
        type Version = AndroidApi;
        const SINCE: u64 = 28;
    }

    /// `Window.getInsetsController` (API 30). Without it the status-bar icons are set with
    /// `View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct InsetsController;

    impl Gate for InsetsController {
        type Version = AndroidApi;
        const SINCE: u64 = 30;
    }

    /// `View.startDragAndDrop` and `Activity.requestDragAndDropPermissions` (API 24). Without them
    /// the window offers no drag source and every drag stays in its window.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct DragSource;

    impl Gate for DragSource {
        type Version = AndroidApi;
        const SINCE: u64 = 24;
    }

    /// `PointerIcon` and `View.setPointerIcon` (API 24). Without them the system
    /// draws its own pointer, which the window cannot change.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct PointerIcons;

    impl Gate for PointerIcons {
        type Version = AndroidApi;
        const SINCE: u64 = 24;
    }

    /// `Window.setBackgroundBlurRadius` and `WindowManager.isCrossWindowBlurEnabled`
    /// (API 31). Without them the system blurs nothing behind a window, and the
    /// window draws no material.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct BackgroundBlur;

    impl Gate for BackgroundBlur {
        type Version = AndroidApi;
        const SINCE: u64 = 31;
    }

    /// `dalvik.system.InMemoryDexClassLoader` (API 26). Without it `FluiView`'s dex is loaded with
    /// a `DexClassLoader` over a read-only copy in the code cache.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct InMemoryDex;

    impl Gate for InMemoryDex {
        type Version = AndroidApi;
        const SINCE: u64 = 26;
    }

    /// `View.setSystemGestureExclusionRects` (API 29). Without it input regions report
    /// `Unavailable` and none is sent to the decor view.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct GestureExclusion;

    impl Gate for GestureExclusion {
        type Version = AndroidApi;
        const SINCE: u64 = 29;
    }

    /// `WindowInsets.getSystemGestureInsets` (API 29). Without it a window reports no system
    /// gesture regions, `NotOnThisDevice`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SystemGestureInsets;

    impl Gate for SystemGestureInsets {
        type Version = AndroidApi;
        const SINCE: u64 = 29;
    }

    /// `VibrationEffect.Composition` with `PRIMITIVE_CLICK` and `PRIMITIVE_TICK`,
    /// `Vibrator.areAllPrimitivesSupported`, and the `CONFIRM`, `REJECT` and `GESTURE_END`
    /// feedback constants (API 30). Without them every haptic pattern falls back to the view's
    /// feedback, which plays the nearest older constant.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct HapticComposition;

    impl Gate for HapticComposition {
        type Version = AndroidApi;
        const SINCE: u64 = 30;
    }

    /// `PRIMITIVE_THUD`, `PRIMITIVE_LOW_TICK` and `Vibrator.getPrimitiveDurations` (API 31).
    /// Without them a pattern that needs a round primitive falls back, and a composition's delays
    /// are the pattern's gaps as they are.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct RoundPrimitives;

    impl Gate for RoundPrimitives {
        type Version = AndroidApi;
        const SINCE: u64 = 31;
    }

    /// `VibrationAttributes.createForUsage` (API 33). Without it a pattern vibration names its
    /// touch usage with sonification `AudioAttributes`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct VibrationTouchUsage;

    impl Gate for VibrationTouchUsage {
        type Version = AndroidApi;
        const SINCE: u64 = 33;
    }

    /// `InputMethodManager.startStylusHandwriting` (API 33). Without it a focused field does not
    /// offer stylus handwriting and a pen press is only a pointer press.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct StylusHandwriting;

    impl Gate for StylusHandwriting {
        type Version = AndroidApi;
        const SINCE: u64 = 33;
    }

    /// The touchpad gesture axes and classifications of `MotionEvent` (API 34). Without them the
    /// window reports no precise scrolling and no trackpad gestures.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct TouchpadGestures;

    impl Gate for TouchpadGestures {
        type Version = AndroidApi;
        const SINCE: u64 = 34;
    }

    /// `Configuration.getLocales` (API 24). Without it the preferred languages are the
    /// configuration's one `locale`.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct LocaleList;

    impl Gate for LocaleList {
        type Version = AndroidApi;
        const SINCE: u64 = 24;
    }

    /// `UiModeManager.getContrast` and its change listener (API 34). Without them high contrast
    /// comes from the high-contrast-text setting alone.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SystemContrast;

    impl Gate for SystemContrast {
        type Version = AndroidApi;
        const SINCE: u64 = 34;
    }

    /// The `DRAG_START`, `SEGMENT_TICK`, `SEGMENT_FREQUENT_TICK` and `GESTURE_THRESHOLD_ACTIVATE`
    /// feedback constants (API 34). Without them the view plays the nearest older constant.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SegmentFeedback;

    impl Gate for SegmentFeedback {
        type Version = AndroidApi;
        const SINCE: u64 = 34;
    }

    /// `VibrationEffect.BasicEnvelopeBuilder` and `Vibrator.areEnvelopeEffectsSupported` (API 36).
    /// Without them a pattern plays as a composition or an amplitude waveform.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct EnvelopeEffects;

    impl Gate for EnvelopeEffects {
        type Version = AndroidApi;
        const SINCE: u64 = 36;
    }
}

/// What the tests of a gate share: each module that declares markers checks them with
/// [`row`](testing::row).
#[cfg(test)]
pub(crate) mod testing {
    use super::{Gate, since};

    /// The failures of one marker's row: the version just before `SINCE` gets no proof, `SINCE`
    /// gets one, and an unknown version gets none.
    pub(crate) fn row<F: Gate>(
        name: &str,
        before: F::Version,
        at: F::Version,
        unknown: F::Version,
    ) -> Vec<String> {
        let mut failures = Vec::new();
        if since::<F>(before).is_some() {
            failures.push(format!("{name}: the version before SINCE got a proof"));
        }
        if since::<F>(at).is_none() {
            failures.push(format!("{name}: SINCE got no proof"));
        }
        if since::<F>(unknown).is_some() {
            failures.push(format!("{name}: an unknown version got a proof"));
        }
        failures
    }
}

#[cfg(test)]
mod tests {
    use super::android::{
        AndroidApi, BackgroundBlur, ContextDisplay, DisplayCutout, DragSource, EnvelopeEffects,
        GestureExclusion, HapticComposition, InMemoryDex, InsetsByType, InsetsController,
        LocaleList, PointerIcons, RoundPrimitives, ScaledScrollFactor, SegmentFeedback,
        StylusHandwriting, SystemContrast, SystemGestureInsets, TextWeight, TouchpadGestures,
        VibrationTouchUsage,
    };
    use super::testing::row;

    /// The markers declared here; the NDK's, the display link's and the Windows display query's
    /// are checked beside them (`owner::android::ndk`, `display::apple::link_rate`,
    /// `display::windows::sample`).
    #[test]
    fn os_version_gate_table() {
        let api = AndroidApi::from_level;
        let unknown = AndroidApi::UNKNOWN;
        let mut failures = Vec::new();
        failures.extend(row::<ContextDisplay>(
            "ContextDisplay",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<TextWeight>("TextWeight", api(30), api(31), unknown));
        failures.extend(row::<LocaleList>("LocaleList", api(23), api(24), unknown));
        failures.extend(row::<SystemContrast>(
            "SystemContrast",
            api(33),
            api(34),
            unknown,
        ));
        failures.extend(row::<ScaledScrollFactor>(
            "ScaledScrollFactor",
            api(25),
            api(26),
            unknown,
        ));
        failures.extend(row::<InsetsByType>(
            "InsetsByType",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<DisplayCutout>(
            "DisplayCutout",
            api(27),
            api(28),
            unknown,
        ));
        failures.extend(row::<InsetsController>(
            "InsetsController",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<DragSource>("DragSource", api(23), api(24), unknown));
        failures.extend(row::<InMemoryDex>("InMemoryDex", api(25), api(26), unknown));
        failures.extend(row::<PointerIcons>(
            "PointerIcons",
            api(23),
            api(24),
            unknown,
        ));
        failures.extend(row::<BackgroundBlur>(
            "BackgroundBlur",
            api(30),
            api(31),
            unknown,
        ));
        failures.extend(row::<GestureExclusion>(
            "GestureExclusion",
            api(28),
            api(29),
            unknown,
        ));
        failures.extend(row::<SystemGestureInsets>(
            "SystemGestureInsets",
            api(28),
            api(29),
            unknown,
        ));
        failures.extend(row::<HapticComposition>(
            "HapticComposition",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<RoundPrimitives>(
            "RoundPrimitives",
            api(30),
            api(31),
            unknown,
        ));
        failures.extend(row::<VibrationTouchUsage>(
            "VibrationTouchUsage",
            api(32),
            api(33),
            unknown,
        ));
        failures.extend(row::<StylusHandwriting>(
            "StylusHandwriting",
            api(32),
            api(33),
            unknown,
        ));
        failures.extend(row::<TouchpadGestures>(
            "TouchpadGestures",
            api(33),
            api(34),
            unknown,
        ));
        failures.extend(row::<SegmentFeedback>(
            "SegmentFeedback",
            api(33),
            api(34),
            unknown,
        ));
        failures.extend(row::<EnvelopeEffects>(
            "EnvelopeEffects",
            api(35),
            api(36),
            unknown,
        ));
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Each environment refuses one step below its floor with the release and the floor named,
    /// starts on the floor, and starts on a version it could not read.
    #[test]
    fn floor_refusal_contract() {
        use flui_platform::{Environment, OsRelease};

        use super::apple::{AppleVersion, MacOSVersion};
        use super::windows::WindowsBuild;
        use super::{OsVersion, refuse_below_floor};
        use crate::PlatformError;

        fn case<V: OsVersion>(
            below: V,
            unknown: V,
            environment: Environment,
            found: &str,
            floor: &str,
        ) -> Vec<String> {
            let mut failures = Vec::new();
            match refuse_below_floor(below) {
                Err(PlatformError::UnsupportedOs {
                    environment: refused,
                    found: reported,
                    floor: stated,
                }) if refused == environment
                    && reported == OsRelease::from_display(found)
                    && stated == OsRelease::from_display(floor) => {}
                other => failures.push(format!("{environment} {below}: {other:?}")),
            }
            for starts in [V::FLOOR_RELEASE, unknown] {
                if let Err(error) = refuse_below_floor(starts) {
                    failures.push(format!("{environment} {starts}: {error}"));
                }
            }
            failures
        }

        let mut failures = Vec::new();
        failures.extend(case(
            WindowsBuild::from_number(9600),
            WindowsBuild::UNKNOWN,
            Environment::Windows,
            "build 9600",
            "build 10240",
        ));
        failures.extend(case(
            AppleVersion::new(12, 5, 7),
            AppleVersion::UNKNOWN,
            Environment::iOS,
            "12.5.7",
            "13.0.0",
        ));
        failures.extend(case(
            MacOSVersion::new(10, 12, 6),
            MacOSVersion::UNKNOWN,
            Environment::MacOS,
            "10.12.6",
            // 10.13 on x86_64, 11.0 on arm64.
            &MacOSVersion::FLOOR_RELEASE.to_string(),
        ));
        failures.extend(case(
            AndroidApi::from_level(22),
            AndroidApi::UNKNOWN,
            Environment::Android,
            "API 22",
            "API 23",
        ));
        // The application's floor (§10): the higher of the two refuses and is the one shown;
        // an application floor below FLUI's changes nothing.
        let api = AndroidApi::from_level;
        for (found, app, refused_at) in [
            (24, Some(26), Some("API 26")),
            (26, Some(26), None),
            (22, Some(21), Some("API 23")),
            (23, Some(21), None),
            (24, None, None),
        ] {
            let outcome = super::refuse_below(api(found), app.map(api));
            let matches = match (&outcome, refused_at) {
                (Ok(()), None) => true,
                (
                    Err(PlatformError::UnsupportedOs {
                        found: reported,
                        floor,
                        ..
                    }),
                    Some(at),
                ) => {
                    *reported == OsRelease::from_display(api(found))
                        && *floor == OsRelease::from_display(at)
                }
                _ => false,
            };
            if !matches {
                failures.push(format!("API {found} with app floor {app:?}: {outcome:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
