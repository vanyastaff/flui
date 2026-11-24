//! FLUI — a Flutter-inspired declarative UI framework for Rust with a
//! `wgpu`-backed GPU rendering engine.
//!
//! This crate is the **facade** over the FLUI workspace: it re-exports the
//! layered crates an application author needs, so a downstream consumer can
//! depend on `flui` alone instead of naming each layer. The framework is not
//! on crates.io: a consumer depends on `flui` by a path to a clone of the
//! repository, or by the git tag that `flui create` pins in a generated
//! project.
//!
//! # Quick start
//!
//! ```no_run
//! use flui::prelude::*;
//!
//! #[derive(Clone, StatelessView)]
//! struct Hello;
//!
//! impl StatelessView for Hello {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Container::new()
//!             .color(Color::rgb(18, 18, 24))
//!             .child(Center::new().child(Text::new("Hello, FLUI!")))
//!     }
//! }
//!
//! fn main() -> Result<(), flui::LaunchError> {
//!     flui::run_app(Hello)
//! }
//! ```
//!
//! The root passed to [`run_app`] is any view, stateless or stateful, as
//! Flutter's `runApp` takes any widget. Create signals in
//! [`view::StatefulView::create_state`] with a [`view::LifecycleContext`], read
//! them during `build`, and write them from event callbacks. The
//! [`view::BuildContext`] deliberately does not provide lifecycle capabilities.
//!
//! # Starting an application
//!
//! [`run_app`]`(root)` starts the same way on every platform: on the desktop
//! it drives the native event loop until shutdown, on iOS it enters UIKit, on
//! the web it starts the browser runner and on Android it runs in the
//! Activity's window. It returns a [`LaunchError`] when the platform cannot
//! start, the system is below FLUI's floor or, on the desktop, the event loop
//! or the first window fails.
//!
//! An application that configures its run builds an [`Application`]: the
//! [`app::AppConfig`] (window configuration and application services) and the
//! capability providers it registers (overrides of the built-in providers and
//! plugins, ADR-0152), ended by [`Application::run`]. `run_app(root)` is
//! `Application::new().run(root)`.
//!
//! A project that also builds for Android marks the function it starts from
//! with [`main`]: the system loads an Android application as a library and
//! calls an exported `android_main`, which the attribute writes, so the
//! application writes no Android code. On every other platform the
//! attribute leaves the function as it is, and the binary's `fn main` calls
//! it:
//!
//! ```no_run
//! # use flui::prelude::*;
//! # #[derive(Clone, StatelessView)]
//! # struct Hello;
//! # impl StatelessView for Hello {
//! #     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView { Text::new("Hello") }
//! # }
//! // src/lib.rs
//! #[flui::main]
//! pub fn run() -> Result<(), flui::LaunchError> {
//!     flui::run_app(Hello)
//! }
//! ```
//!
#![cfg_attr(
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ),
    doc = "On the desktop, [`Application::on_ready`] gives an [`app::AppHandle`] for
cross-thread show and quit requests, and [`Application::run_resident`] starts an
application without a window, building each main window it later shows from a
factory. A successful show-command admission is not window completion: await its
returned request or poll `try_result` to obtain the window result."
)]
//!
//! # Choosing features
//!
//! The base surface — [`widgets`], [`view`], [`animation`], [`run_app`], and
//! the non-Material half of [`prelude`] — needs no feature at all; that
//! includes the global widgets localizations in [`widgets`]. Nothing is on by
//! default (ADR-0088 §6): an application names what it uses, for example
//! `features = ["material"]`. The table lists the manifest's features in its
//! order:
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `testing` | off | `flui::testing`, the deterministic headless test driver, with the render-object harness and the scripted capability scenarios of [`platform::capability`]; for a development dependency |
//! | `animation-diagnostics` | off | structured animation lifecycle events and CPU spans in `flui-animation` |
//! | `material` | off | `flui::material` and the Material half of [`prelude`] |
//! | `cupertino` | off | `flui::cupertino` |
//! | `material-icons` | off | the Material Icons face in the binary, for an application that draws the family `"Material Icons"` with [`widgets::Icon`] without the `material` catalog, which brings the face itself |
//! | `cupertino-icons` | off | the Cupertino Icons face (family `"CupertinoIcons"`) in the binary, for an application that draws that family with [`widgets::Icon`]; the `cupertino` catalog draws no glyph of it and does not bring it, as Flutter takes the face from the `cupertino_icons` package |
//! | `system` | off | `flui::services::system`: the launcher, file-dialog, autostart, global-shortcut, progress and user-folder providers |
//! | `http` | off | the built-in `HttpCapability` provider, each host's own HTTP stack (`ureq` over rustls on Linux and the BSDs); off, an `HttpCapability` request is refused as not registered and no HTTP client or TLS stack is linked |
//! | `images` | off | `flui::assets` and the widgets' `Image::asset` and `Image::network`, with the decoded-image cache each UI runtime keeps; a network image also needs `http`; off, no image decoder is linked |
//! | `single-instance` | off | one instance per user session on Linux and Windows (`Application::single_instance`): a later launch hands the running instance its links and files; off, each launch is a process of its own, and `flui build` and `flui run` refuse a desktop build of an application that declares URL schemes or file types |
//! | `unstable-input` | off | `flui::platform::input_capabilities` and `flui::platform::haptics`, the unstable input vocabulary (ADR-0213 §5, ADR-0210 §9) |
//! | `native` | off | `flui::native`, the operating systems' window extensions (Evolving, ADR-0089 §5) |
//! | `unstable-windows` | off | `app::open_window`, `app::open_secondary_window` and `app::WindowPolicy`: windows beyond the first (unstable) |
//! | `devtools` | off | `flui::devtools`: the frame profiler, timeline and inspector counters |
//! | `driver` | off | the driver agent in an optimized build; every debug build serves its windows to a driver launched with an endpoint (ADR-0239) |
//! | `serde` | off | `serde` implementations for the framework's value types: geometry, paint and style, locale and brightness, gesture details, layout enums, curves and stored document revisions |
//!
//! A module whose feature is off is **absent**, not empty: `flui::cupertino`
//! without the `cupertino` feature is an unresolved-import error at the use
//! site. Native accessibility is not a feature: on Windows, macOS and Linux
//! every window offers its semantics tree to the operating system's assistive
//! technology through AccessKit. Hot reload is the official package
//! `flui-hot-reload`, which an application adds as a dependency of its own and
//! installs with `app::AppConfig::dev_reload`; the facade re-exports no
//! package for it (ADR-0311 §6).
//!
//! `native` and `unstable-windows` are absent without their feature:
//!
#![cfg_attr(not(feature = "native"), doc = "```compile_fail,E0432")]
#![cfg_attr(feature = "native", doc = "```")]
//! use flui::native::HostWindow;
//! ```
//!
#![cfg_attr(not(feature = "unstable-windows"), doc = "```compile_fail,E0432")]
#![cfg_attr(feature = "unstable-windows", doc = "```no_run")]
//! use flui::app::WindowPolicy;
//! ```
//!
//! # Using a design system
//!
//! [`prelude`] curates the everyday `flui-widgets` surface plus — when the
//! `material` feature is on — the common Material widgets (see [`prelude`]'s
//! own docs for the curation rule); the full catalogs live at `flui::material`
//! and `flui::cupertino`:
//!
#![cfg_attr(
    feature = "material",
    doc = r#"```no_run
use flui::material::{AppBar, Scaffold, Theme, ThemeData};
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct MaterialHello;

impl StatelessView for MaterialHello {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            Scaffold::new()
                .app_bar(AppBar::new().title(Text::new("FLUI")))
                .body(Center::new().child(Text::new("Hello, Material!"))),
        )
    }
}

fn main() -> Result<(), flui::LaunchError> {
    flui::run_app(MaterialHello)
}
```"#
)]
//!
//! # Layers
//!
//! Each module re-exports one workspace crate, or a curated part of it. The
//! layering (no upward edges) is in `docs/architecture.md`.
//!
//! | Module | Crate | Feature | Layer |
//! |---|---|---|---|
//! | [`geometry`] | `flui-foundation` | — | geometry values (`f64` logical pixels) |
//! | [`foundation`] | `flui-foundation` | — | keys, listenables, ids, diagnostics |
//! | [`painting`] | `flui-painting`, `flui-rendering`, `flui-semantics` | — | custom painting: `CustomPainter`, `CustomPaintContext`, `Canvas`, paint, style and text values |
//! | [`platform`] | `flui-platform` | — | platform contracts and values (brightness, locale, windows, storage, IME) |
//! | [`view`] | `flui-view` | — | View/Element tree |
//! | [`widgets`] | `flui-widgets` | — | user-facing widget catalog |
//! | [`animation`] | `flui-animation` | — | curves, tweens, tickers |
//! | [`scheduler`] | `flui-scheduler` | — | the frame scheduler's handles and timing values |
//! | [`rendering`] | `flui-rendering`, `flui-objects` | — | custom render objects; the built-in ones in `rendering::objects` |
//! | [`interaction`] | `flui-interaction` | — | gesture recognizers, the gesture arena, hit testing and focus vocabulary |
//! | [`semantics`] | `flui-semantics` | — | the vocabulary for describing a node to assistive technology |
//! | [`app`] | `flui-app` | — | configuration, background work, close requests, frame failures |
//! | `material` | `flui-material` | `material` | Material Design theming and widget catalog |
//! | `cupertino` | `flui-cupertino` | `cupertino` | iOS-style theming and widget catalog |
//! | `native` | `flui-app` (`flui-native` behind it) | `native` | one operating system's window extensions |
//! | `services` | `flui-system` | `system` | OS services from capability crates; `services::system` |
//! | `assets` | `flui-assets` | `images` | the decoded-image cache |
//! | `devtools` | `flui-devtools` | `devtools` | profiler, timeline and inspector counters |
//! | `testing` | `flui-testing`, `flui-rendering` | `testing` | headless application and render-object tests |
//!
//! [`painting`], [`rendering`], [`interaction`], [`semantics`] and [`app`] are
//! authoring modules: they name the types an application, a custom painter, a
//! render object or a gesture recognizer signs. The runtime's owners appear
//! only where a kept signature names them, and platform implementations stay
//! outside. `flui::material` and `flui::cupertino` sit above [`widgets`]
//! (ADR-0028: `material --> widgets`, `cupertino --> widgets`, never the
//! reverse), which is why `flui` is on that ADR's allowlist of crates that
//! may depend on both.

// Every public facade item is documented; keep it that way.
#![deny(missing_docs)]

pub mod app;
mod book;
pub mod interaction;
pub mod painting;
pub mod rendering;
pub mod semantics;
#[cfg(feature = "testing")]
pub mod testing;

/// The animation layer (`flui-animation`): every public item of the crate, at
/// the facade's path, as the same items.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_animation::__runtime`, the frame driver's seam (ADR-0382), which is
/// not application surface: an application receives a `Vsync` handle from its
/// tree and never ticks or closes a registry.
///
/// ```compile_fail,E0603
/// use flui::animation::__runtime::VsyncDriver;
/// ```
pub mod animation {
    pub use flui_animation::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0382): the frame driver's seam is not facade surface"
    )]
    mod __runtime {}
}

/// The decoded-image cache each UI runtime keeps for `Image::asset` and
/// `Image::network` (`flui-assets`): `ImageCacheCapability` to provide another.
/// Requires the `images` feature.
#[cfg(feature = "images")]
pub use flui_assets as assets;
/// The iOS-style design system (`flui-cupertino`). Requires the `cupertino`
/// feature.
#[cfg(feature = "cupertino")]
pub use flui_cupertino as cupertino;
/// The profiler, timeline and inspector counters (`flui-devtools`). Requires the `devtools`
/// feature.
#[cfg(feature = "devtools")]
pub use flui_devtools as devtools;
/// The values every layer shares: keys, listenables, the ids and errors the
/// trees report, text direction and sizing, the application id and the typed
/// self-description of a node.
///
/// The rest of `flui-foundation` (the frame arena, claim slots, id mints, slot
/// tables, the signal graph's read side, the rest of panic containment) is how the
/// framework is built, not what an application names; an official package
/// reaches it through `flui-sdk`. The arity markers are in [`rendering`],
/// geometry in [`geometry`].
pub mod foundation {
    pub use flui_foundation::{
        AsViewKey, BuildPhase, BuildPhaseGuard, ChangeNotifier, CreationSite, DataTransferId,
        DragSessionId, ElementId, FrameCallbackId, FrameEpoch, FrameStamp, GenId, Id, IdExhausted,
        Identifier, InputSequenceId, LayerId, Listenable, ListenerCallback, ListenerId,
        ListenerObserver, ManualClock, Marker, MintedId, MonotonicClock, MutatedDuringBuild,
        NotificationCallback, Notifier, NotifierCallback, PathEntry, PresentationAddress,
        PresentationId, PressId, RawId, RebuildReason, RebuildReasons, RecoveryCallback, RenderId,
        SemanticsId, Subscription, SurfaceGeneration, TextDirection, TextScaleProfile, TextSize,
        TextSizeError, TextSizeRequest, TextSizingIntent, TreeContext, TreeError, TreeId,
        UiRuntimeId, UniqueKey, ValueKey, ValueListenable, ValueNotifier, ViewId, ViewKey, inspect,
        markers, observe,
    };

    /// A slot's typed generation, as the ids above carry it.
    pub mod generation {
        pub use flui_foundation::generation::{Generation, ReservedGenerationBit};
    }

    /// A holder of memory it can give back when the system asks.
    pub mod memory {
        pub use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
    }

    /// The scope an observer of a notification runs user code in, so a panic is caught once.
    pub mod panic {
        pub use flui_foundation::panic::RecoveryScope;
    }
}
pub use flui_foundation::geometry;
/// The Material Design system (`flui-material`). Requires the `material`
/// feature.
#[cfg(feature = "material")]
pub use flui_material as material;
/// The frame scheduler's values (`flui-scheduler`) a widget's state holds or
/// receives: the asynchronous driver and its task tokens, the timer and its
/// sleeps, the post-frame handle, a frame's timing and phase, and the waker
/// that requests a frame.
///
/// A state acquires the handles from its `LifecycleContext` (`async_driver`,
/// `timer`, `post_frame_handle`).
pub mod scheduler {
    /// The frame scheduler a [`PostFrameHandle`] is made from.
    pub use flui_scheduler::UpdateScheduler;
    pub use flui_scheduler::{
        AsyncDriver, BoxedTask, FrameId, FramePhase, FrameTiming, FrameWaker, Instant,
        PostFrameHandle, PostFrameScheduleError, Sleep, TaskToken, TimerHandle,
    };
}
/// OS services from capability crates (ADR-0154 §8), each behind the feature
/// of its name.
///
/// `system` (`flui-system`): the providers of the launcher, whose handle is
/// [`platform::launch::Launcher`], the file dialogs, autostart, global
/// shortcuts, the launcher entry's progress and count, and the user's
/// folders. An application registers `services::system::SystemPlugin`.
#[cfg(feature = "system")]
pub mod services {
    pub use flui_system as system;
}
/// What one operating system's windows offer beyond the window contract
/// (ADR-0254 §8). Requires the `native` feature; Evolving, not Stable
/// (ADR-0089 §5).
///
/// A widget reaches the window it is presented in as a capability: it acquires
/// a [`NativeWindow`](native::NativeWindow) with
/// `ctx.capability::<NativeWindowCapability>()` in `create_state`, keeps it in
/// its state, and runs a closure on the window with
/// [`NativeWindow::with`](native::NativeWindow::with) from a callback.
/// [`with_window`](native::with_window) does the same by the address a window
/// request resolves to or a close request names. Each system's module has an
/// extension trait implemented for `dyn HostWindow`, the window as the core
/// host hands it out.
/// Its one method narrows the window to that system's view of it (`win32()`,
/// `appkit()`) and answers `None` for a window of any other host (headless,
/// another system), so nothing is asked of a window that cannot take it
/// and nothing panics. The views take and return FLUI types; a raw window or
/// display handle leaves only through `raw-window-handle`'s `HasWindowHandle`
/// and `HasDisplayHandle`, which every window implements.
///
/// What a window is created with on one system is a value of that system's
/// type in [`platform::os_options::OsWindowOptions`], given to
/// [`app::AppConfig::os_options`].
#[cfg(feature = "native")]
pub mod native {
    #[cfg(target_os = "macos")]
    pub use flui_app::native::macos;
    #[cfg(windows)]
    pub use flui_app::native::windows;
    pub use flui_app::native::{HostWindow, NativeWindow, NativeWindowCapability, with_window};
}
/// What the system reports and offers (`flui-platform`): its preferences,
/// locale and displays, the windows an application opens, storage, launch and
/// the clipboard, and the host contracts other facade signatures name. Each
/// item has one path: an item of one of `flui-platform`'s modules is named
/// through that module, and the input vocabulary widgets use is in
/// [`interaction`]. A package reaches the rest of the crate through
/// `flui-sdk`.
///
/// [`input_capabilities`](flui_platform::input::capabilities) and
/// [`haptics`](flui_platform::haptics), the second haptic tier with its
/// requests and outcomes, are unstable and present only under the additive
/// `unstable-input` feature (ADR-0213 §5, ADR-0210 §9). Without it the modules
/// do not exist:
///
#[cfg_attr(not(feature = "unstable-input"), doc = "```compile_fail,E0432")]
#[cfg_attr(feature = "unstable-input", doc = "```")]
/// use flui::platform::input_capabilities::InputCapabilities;
/// ```
///
#[cfg_attr(not(feature = "unstable-input"), doc = "```compile_fail,E0432")]
#[cfg_attr(feature = "unstable-input", doc = "```")]
/// use flui::platform::haptics::HapticPattern;
/// ```
pub mod platform {
    pub use flui_platform::{
        AdmissionError, AdmittedCommand, AppDataDir, AppTitleBar, ApplyError, AspectRatio,
        Availability, Brightness, BrowserException, CapabilityAbsence, CapabilityParts,
        ClipboardEntryId, ClipboardError, ClipboardExpiry, ClipboardExpiryError, ClipboardRead,
        ClipboardWrite, ClipboardWritten, ColorGamut, CommandRefusal, Concealment, ControlsOffset,
        CopiedAside, Cursor, CursorError, CursorGrab, CursorImage, CursorImageError, DesktopOrigin,
        DisplayBounds, DisplayColor, DisplayDescription, DisplayId, DisplayParts, DisplayRevision,
        DisplayRevisionExhausted, DisplaySnapshot, Distance, DragDropEvent, DurationScale,
        DxgiFrameStatistics, DxgiShown, DxgiStatistics, DynamicRange, EmptyPointerFrame,
        Environment, ExpiryClearing, FlingSpeeds, FramePacing, FramePresented, FrameRateHint,
        FrameRequest, GestureGeometry, GestureParts, GesturePreferences, HapticFeedback, Headroom,
        HostNotice, HourCycle, ImeEvent, InputLatency, InsetCurve, InsetMotion, InvalidAppDataDir,
        InvalidControlsOffset, InvalidDisplayBounds, InvalidGeometry, InvalidHeadroom,
        InvalidInsets, InvalidLocale, InvalidPreference, InvalidRefreshPeriod,
        InvalidRegionalFormat, InvalidWindowOption, KeyboardPlacement, Launch, Locale,
        MAX_OPENED_URL_LEN, MAX_SURFACE_EXTENT, MeasurementSystem, MotionPreference,
        NativeMouseGeometry, NativeTouchGeometry, NumberingSystem, OpenTarget, Opened, OpenedFile,
        OpenedOverflow, OpeningGeometry, OsRelease, PlacementError, PlacementOffset,
        PlatformFailure, PlatformInput, PlatformSystemChrome, PlatformTextInput, PlatformWindow,
        PointerFrame, PreferenceParts, PreferenceQueryError, PublicationMint, RedrawRequester,
        RefreshPeriod, RegionOverride, RegionalFormats, RegionalParts, ResolvedPlacement,
        ScrollBarParts, ScrollBarPreferences, ScrollBarVisibility, ScrollTrackClick,
        SessionEndAnswer, SessionEndBlock, SessionEndBlockError, SessionEndRequest,
        SizeConstraints, SoftKeyboard, SrgbColor, Storage, StorageError, StorageFuture,
        StorageName, Stored, StoredVersion, SubmitError, SurfaceColorSpace, SurfaceTarget,
        SwapChainStatistics, SystemControls, SystemFailure, SystemNotice, SystemPreferences,
        SystemUiOverlayStyle, TargetPlatform, TextWeightPreference, Unsupported, UnsupportedReason,
        Weekday, WheelParts, WheelPreferences, WheelStep, WindowAt, WindowCapabilities,
        WindowCapability, WindowCommand, WindowDisplay, WindowExecutionState, WindowExtent,
        WindowFocus, WindowFrame, WindowGeometry, WindowId, WindowInsets, WindowInteraction,
        WindowLevel, WindowMaterial, WindowMode, WindowOption, WindowOptions, WindowPlacement,
        WindowPosition, WindowReveal, WindowRevision, WindowRevisionExhausted, WindowState,
        WindowStateParts, WriteMode,
    };
    /// Capabilities: what an application registers, and the contracts a provider and a host meet.
    pub mod capability {
        pub use flui_platform::capability::{
            Abandoned, Activity, ActivityBridge, ActivityResult, Capability, CapabilityId,
            CapabilityProvider, CapabilityRegistrar, Completion, CompletionSource, DataRefused,
            HostBridge, IntentFrame, InvalidResultKey, JniFrame, MAX_POSTED_WORK,
            MainThreadRefused, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions,
            Plugin, PortalParent, PostedWork, ProviderContext, RegisteredProvider, RequestKey,
            Resolver, ResultCode, ResultIntent, ResultIntentHold, ResultKey, ResultMint,
            ResultOutcome, ResultRefused, ResultRequest, ResultTicket, ResultTicketHold,
            ResultToken, RuntimeLocals, WindowCloseWatch, WindowLifetime, WindowRef,
        };
        /// Scripted capability scenarios for tests; with the `testing` feature.
        #[cfg(feature = "testing")]
        pub mod scripted {
            pub use flui_platform::capability::scripted::{
                AlreadyDriven, Answer, EventSubscription, Events, Expect, FailureReport,
                RequestTicket, Scenario, ScenarioDriver, ScenarioFailure, ScenarioFailures, Script,
                ServiceState, StreamEnd, StreamItem, Track,
            };
        }
    }
    /// Data a drag or a paste carries, in the formats its source offers.
    pub mod data_transfer {
        pub use flui_platform::transfer::data::{
            DataTransferOffer, DragTransport, DropFeedback, Mime, RepresentationDescriptor,
            RepresentationIndex, TransferCompleter, TransferError, TransferFormat, TransferLimits,
            TransferPayload, TransferRequest,
        };
    }
    /// An operating-system drag session, as the host runs it.
    pub mod drag {
        pub use flui_platform::transfer::drag::{
            DragRequestError, DragSessionKey, OsDragEnd, OsDragEndHook, OsDragParts,
            OsDragProvider, OsDragRequest, OsDragStart, OsDragTerms, PlatformDragSource,
            ProcessNonce,
        };
    }
    /// Where background work runs.
    pub mod executor {
        pub use flui_platform::executor::{Executor, Lane, RoomFuture};
    }
    /// HTTP requests through the system's stack (ADR-0280).
    pub mod http {
        pub use flui_platform::http::{
            BodySource, HeaderName, HeaderValue, Headers, Http, HttpAuthority, HttpClient,
            HttpError, HttpFuture, HttpRequest, HttpResponse, InvalidHeader, Method, ResponseBody,
            StatusCode,
        };
    }
    /// The regions of a window the system treats as caption, buttons or resize edges.
    pub mod input_regions {
        pub use flui_platform::input::regions::{
            CaptionButton, InputRegion, InputRegionKind, ResizeEdge,
        };
    }
    /// Keyboard input beyond the keys `flui::interaction` names: composition and produced text.
    pub mod keyboard {
        pub use flui_platform::input::keyboard::{ImeComposition, KeyText};
    }
    /// Opening a URI or a path with the system's handler.
    pub mod launch {
        pub use flui_platform::launch::{
            ExternalUri, InvalidUri, LaunchError, LaunchResult, Launcher, LauncherBackend,
            MAX_EXTERNAL_URI_LEN, REFUSED_SCHEMES, Scheme,
        };
    }
    /// Pointer events as the host delivers them; the vocabulary widgets use is in `flui::interaction`.
    pub mod pointer {
        pub use flui_platform::input::pointer::{
            ButtonDirection, ContactSize, HoverDistance, InputValueError, InvalidButtonNumber,
            MismatchedPointerSource, MotionAcceleration, MotionDelta, PenOrientation,
            PointerButtonEvent, PointerCancel, PointerDeviceChange, PointerSignal, Press, Pressure,
            Quantity, RawMotion, Release, SmartZoomEvent, SwipeOffset, TangentialPressure,
            TouchpadHoldEvent, TouchpadHoldPhase, TouchpadSwipeEvent, TouchpadSwipePhase, Twist,
        };
    }
    /// The text store a text field shares with the system's input method.
    pub mod text_store {
        pub use flui_platform::text::store::{
            Autofill, Capitalization, CommitGate, Composition, CompositionEnd, Correction,
            EnterAction, LockGrant, LockKind, LockOutcome, LockTiming, NumberFormat, OwnerCalls,
            PointMode, RangeRect, Selection, TextChange, TextInputOptions, TextInputPurpose,
            TextStore, TextStoreChange, TextStoreEdit, TextStoreError, TextStoreHost,
            TextStoreHostError, TextStoreObserver, TextStoreRead, TextStoreStatus,
        };
        /// UTF-16 offsets, the unit the input method counts in.
        pub mod utf16 {
            pub use flui_platform::text::store::utf16::{OffsetError, Utf16Offset, Utf16Range};
        }
    }
    pub use flui_platform::window::os_options;
    /// Handle-less input capabilities (ADR-0213); unstable.
    #[cfg(feature = "unstable-input")]
    pub mod input_capabilities {
        pub use flui_platform::input::capabilities::{InputCapabilities, InputCapabilitiesBuilder};
    }
    /// Haptic patterns, requests and outcomes (ADR-0210); unstable.
    #[cfg(feature = "unstable-input")]
    pub mod haptics {
        pub use flui_platform::haptics::{
            ControlPoint, HapticCapabilities, HapticEvent, HapticOutcome, HapticPattern,
            HapticPatternBuilder, HapticPatternError, HapticRequest, HapticTicket, HapticTiming,
            Intensity, ParameterCurve, PatternPlayback, PlatformHaptics, Sharpness, UnitRangeError,
        };
    }
}
pub use flui_widgets as widgets;

/// The view layer (`flui-view`): View/Element, `BuildContext`, keys and
/// signals.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_view::__runtime`, the composition roots' seam (ADR-0081 §4), which
/// is not application surface:
/// `thread_boundary_ui` pins the module-privacy diagnostic alongside valid
/// facade view and internal composition-root callers.
///
/// ```compile_fail,E0603
/// use flui::view::__runtime;
/// ```
pub mod view {
    pub use flui_view::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0081 §4): the composition roots' seam is not facade surface"
    )]
    mod __runtime {}
}

/// The function an application starts from on every platform; on Android it
/// writes the `android_main` the system calls. Re-exported from `flui-app`.
pub use flui_app::main;
/// Run `root` in a main window: `Application::new().run(root)`, as Flutter's
/// `runApp` takes the root widget. Re-exported from `flui-app`.
///
/// The root is any [`view::View`], stateless or stateful. It returns a
/// [`LaunchError`] when the platform cannot start, the system is below FLUI's
/// floor or the run fails; on the desktop it returns after shutdown.
pub use flui_app::run_app;
/// The builder of a configured run, and why a run fails. Re-exported from
/// `flui-app`; the configuration it takes and the rest of the application's
/// vocabulary are in [`app`].
pub use flui_app::{Application, LaunchError};

/// Everything an application author needs in scope to write widget code:
/// the widget catalog prelude, [`run_app`], and — with the `material` feature
/// on — the everyday Material widgets.
///
/// # Two halves
///
/// The **base half** is [`flui_widgets::prelude`] plus [`run_app`],
/// [`Application`] and [`LaunchError`], and is always present. It is
/// design-system-neutral, so an application that selects no catalog still
/// writes ordinary screens off `use flui::prelude::*`.
///
/// The **Material half** is `#[cfg(feature = "material")]`. Turning the
/// feature off removes those names from the glob rather than replacing them
/// with stubs; a build that was relying on them fails at the use site.
///
/// # Curation rule (Material half)
///
/// This module answers "what does *every* app touch", not "what does
/// `flui-material` export". A type belongs here when an app author reaches
/// for it while writing ordinary screens: layout/text/interaction primitives
/// (via [`flui_widgets::prelude`]), theming (`Theme`, `ThemeData`,
/// `ColorScheme`, `TextTheme`), the app shell (`Scaffold`, `AppBar`,
/// `Drawer`), the standard button family, and the common
/// data-display/feedback/navigation widgets a Material screen composes
/// (`Card`, `ListTile`, `Dialog`, `SnackBar`, `NavigationBar`, the tab
/// family, …).
///
/// It does **not** include `*State` handles an app never constructs
/// directly (`InkWellState`, `TabBarState`, …), component-theme override
/// structs (`AppBarThemeData` and friends: per-widget customization, reached
/// at `flui::material`), or `flui-material` internals such as the raw M3
/// type-scale table (`english_like_2021`).
///
/// **`TextField` is the Material type.** The theme-free text-editing
/// primitive of `flui-widgets` is [`flui_widgets::RawTextField`], so the glob
/// never holds two `TextField`s and what `TextField` means does not depend on
/// whether the `material` feature is on. The Cupertino catalog has no such
/// collision (every type is `Cupertino`-prefixed), but its surface is
/// app-shell-shaped (`CupertinoPageScaffold`, `CupertinoTabScaffold`, …), so
/// it stays at `flui::cupertino` rather than joining this glob.
pub mod prelude {
    pub use flui_app::{Application, LaunchError, run_app};
    #[cfg(feature = "material")]
    pub use flui_material::{
        AlertDialog, AppBar, BackButton, Card, Checkbox, Chip, ColorScheme, DefaultTabController,
        Dialog, Divider, Drawer, ElevatedButton, FilledButton, FilterChip, FloatingActionButton,
        IconButton, InkWell, ListTile, Material, MaterialApp, NavigationBar, NavigationDestination,
        OutlinedButton, Radio, Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle,
        ScaffoldMessengerScope, SnackBar, Switch, Tab, TabBar, TabBarView, TabController,
        TextButton, TextField, TextFormField, TextTheme, Theme, ThemeData, ThemeMode,
        VerticalDivider, show_dialog,
    };
    pub use flui_widgets::prelude::*;
}
