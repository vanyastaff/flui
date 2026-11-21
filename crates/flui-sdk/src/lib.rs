//! The package-author surface of FLUI (ADR-0088 §4).
//!
//! A package such as a design system depends on this crate instead of on the
//! internal crates one by one, and it never pulls in the host, the engine or
//! the GPU stack: `flui-app`, `flui-engine` and `wgpu` are outside its normal
//! dependency closure.
//!
//! The crate is **Evolving**: it has its own `0.N` version, bumped on every
//! train, and its surface may change on any train without a `flui` major.
//!
//! - **Whole modules** at the facade's paths: [`animation`], [`foundation`],
//!   [`geometry`] and [`widgets`] are the internal crates (or, for
//!   `geometry`, foundation's module) themselves, so `flui_sdk::widgets::Text`
//!   and `flui::widgets::Text` are one type. [`view`] is a glob of the whole
//!   `flui-view` crate that shadows its `__runtime` seam (ADR-0081 §4).
//! - **Curated modules** at the facade's paths: [`interaction`], [`painting`],
//!   [`platform`], [`rendering`], [`scheduler`] and [`semantics`] hold the subset of the
//!   facade's module that packages use, as the same items, not wrappers.
//! - **Evolving modules** [`pipeline`] and [`hooks`]: render-object internals
//!   and development hooks the facade does not expose. A package's exposure to
//!   them is `grep -E 'flui_sdk::(pipeline|hooks)::'`.
//!
//! Each item is here because `flui-material`, `flui-cupertino` or
//! `flui-devtools` imports it outside its tests, or because an approved
//! feature spec places it in the package-author surface (the input
//! completeness spec's placement table: arena observation, gesture presets,
//! the gesture slots' locks, the swipe recognizer, system preferences, haptic
//! feedback kinds and input regions);
//! `tests/surface.rs` pins the list.

// Derive expansions use the same absolute owner path in library and integration targets.
#[allow(
    unused_extern_crates,
    reason = "derive expansions resolve the owner by its absolute crate name"
)]
extern crate self as flui_sdk;

/// The animation layer (`flui-animation`): every public item of the crate, at
/// the SDK's path, as the same items.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_animation::__runtime`, the frame driver's seam (ADR-0382), which is
/// not package-author surface: a package receives a `Vsync` handle from its
/// tree and never ticks or closes a registry.
///
/// ```compile_fail,E0603
/// use flui_sdk::animation::__runtime::VsyncDriver;
/// ```
pub mod animation {
    pub use flui_animation::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0382): the frame driver's seam is not SDK surface"
    )]
    mod __runtime {}
}

pub use flui_foundation as foundation;
pub use flui_foundation::geometry;
pub use flui_widgets as widgets;

/// The view layer (`flui-view`): every public item of the crate, at the
/// facade's path, as the same items.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_view::__runtime`, the composition roots' seam (ADR-0081 §4), which
/// is not package-author surface:
///
/// ```compile_fail,E0603
/// use flui_sdk::view::__runtime::BindingRuntime;
/// ```
///
/// The exact module-privacy diagnostic is pinned by `trybuild_ui::ui_tests`
/// in `tests/compiler_guards.rs`, alongside a compiling composition-root path.
pub mod view {
    pub use flui_view::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0081 §4): the composition roots' seam is not SDK surface"
    )]
    mod __runtime {}
}

/// The frame scheduler's handles packages hold, at the facade's path: the
/// asynchronous driver and the token of a task it runs, the post-frame handle,
/// and the waker that requests a frame.
pub mod scheduler {
    pub use flui_scheduler::AsyncDriver;
    pub use flui_scheduler::FrameWaker;
    pub use flui_scheduler::PostFrameHandle;
    pub use flui_scheduler::TaskToken;
}

/// Platform values: brightness, locale, a clipboard read's answer and a write, haptic
/// feedback kinds, the host's system preferences (its haptics setting among
/// them) and every type its accessors return, the target platform (the one a
/// gesture preset models), the window's state and mode, and the system
/// notices the host delivers.
pub mod platform {
    pub use flui_platform::AdmissionError;
    pub use flui_platform::Brightness;
    pub use flui_platform::ClipboardEntryId;
    pub use flui_platform::ClipboardError;
    pub use flui_platform::ClipboardExpiry;
    pub use flui_platform::ClipboardExpiryError;
    pub use flui_platform::ClipboardRead;
    pub use flui_platform::ClipboardWrite;
    pub use flui_platform::Concealment;
    pub use flui_platform::DurationScale;
    pub use flui_platform::ExpiryClearing;
    pub use flui_platform::GesturePreferences;
    pub use flui_platform::HapticFeedback;
    pub use flui_platform::HostNotice;
    pub use flui_platform::HourCycle;
    pub use flui_platform::InvalidLocale;
    pub use flui_platform::InvalidPreference;
    pub use flui_platform::InvalidRegionalFormat;
    pub use flui_platform::Locale;
    pub use flui_platform::MeasurementSystem;
    pub use flui_platform::MotionPreference;
    pub use flui_platform::NativeMouseGeometry;
    pub use flui_platform::NativeTouchGeometry;
    pub use flui_platform::NumberingSystem;
    pub use flui_platform::RegionOverride;
    pub use flui_platform::RegionalFormats;
    pub use flui_platform::ScrollBarPreferences;
    pub use flui_platform::ScrollBarVisibility;
    pub use flui_platform::ScrollTrackClick;
    pub use flui_platform::SessionEndBlock;
    pub use flui_platform::SessionEndRequest;
    pub use flui_platform::SrgbColor;
    pub use flui_platform::SubmitError;
    pub use flui_platform::SystemPreferences;
    pub use flui_platform::TargetPlatform;
    pub use flui_platform::TextWeightPreference;
    pub use flui_platform::Unsupported;
    pub use flui_platform::UnsupportedReason;
    pub use flui_platform::Weekday;
    pub use flui_platform::WheelPreferences;
    pub use flui_platform::WheelStep;
    pub use flui_platform::WindowMode;
    pub use flui_platform::WindowState;

    /// The text store a text field shares with the system's input method.
    pub mod text_store {
        pub use flui_platform::text::store::Autofill;
        pub use flui_platform::text::store::Capitalization;
        pub use flui_platform::text::store::Correction;
        pub use flui_platform::text::store::EnterAction;
        pub use flui_platform::text::store::NumberFormat;
        pub use flui_platform::text::store::TextInputOptions;
        pub use flui_platform::text::store::TextInputPurpose;
    }

    /// The capability seam (ADR-0152): what a package declares, provides and
    /// registers a platform capability with, and the owner-thread result of a
    /// call the operating system finishes later. A widget acquires a
    /// capability with `view::LifecycleContextExt::capability`.
    pub mod capability {
        pub use flui_platform::capability::Abandoned;
        pub use flui_platform::capability::Activity;
        pub use flui_platform::capability::ActivityResult;
        pub use flui_platform::capability::Capability;
        pub use flui_platform::capability::CapabilityProvider;
        pub use flui_platform::capability::CapabilityRegistrar;
        pub use flui_platform::capability::Completion;
        pub use flui_platform::capability::CompletionSource;
        pub use flui_platform::capability::DataRefused;
        pub use flui_platform::capability::IntentFrame;
        pub use flui_platform::capability::InvalidResultKey;
        pub use flui_platform::capability::JniFrame;
        pub use flui_platform::capability::MainThreadRefused;
        pub use flui_platform::capability::OwnerPoster;
        pub use flui_platform::capability::OwnerRetired;
        pub use flui_platform::capability::Plugin;
        pub use flui_platform::capability::ProviderContext;
        pub use flui_platform::capability::Resolver;
        pub use flui_platform::capability::ResultCode;
        pub use flui_platform::capability::ResultIntent;
        pub use flui_platform::capability::ResultKey;
        pub use flui_platform::capability::ResultOutcome;
        pub use flui_platform::capability::ResultRefused;
        pub use flui_platform::capability::ResultRequest;
        pub use flui_platform::capability::ResultTicket;
        pub use flui_platform::capability::ResultToken;
        pub use flui_platform::capability::WindowRef;
    }

    /// Handle-less input capabilities (ADR-0213). Unstable: present only
    /// under the additive `unstable-input` feature.
    #[cfg(feature = "unstable-input")]
    pub mod input_capabilities {
        pub use flui_platform::Availability;
        pub use flui_platform::input::capabilities::InputCapabilities;
        pub use flui_platform::input::capabilities::PointerKindSet;
    }

    /// Haptic patterns, requests and outcomes (ADR-0210); the widget handle
    /// that takes them is `interaction::HapticsHandle`, as in the facade.
    /// Unstable: present only under the additive `unstable-input` feature.
    #[cfg(feature = "unstable-input")]
    pub mod haptics {
        pub use flui_platform::haptics::ControlPoint;
        pub use flui_platform::haptics::HapticCapabilities;
        pub use flui_platform::haptics::HapticEvent;
        pub use flui_platform::haptics::HapticOutcome;
        pub use flui_platform::haptics::HapticPattern;
        pub use flui_platform::haptics::HapticPatternBuilder;
        pub use flui_platform::haptics::HapticPatternError;
        pub use flui_platform::haptics::HapticRequest;
        pub use flui_platform::haptics::HapticTicket;
        pub use flui_platform::haptics::HapticTiming;
        pub use flui_platform::haptics::Intensity;
        pub use flui_platform::haptics::ParameterCurve;
        pub use flui_platform::haptics::PatternPlayback;
        pub use flui_platform::haptics::Sharpness;
        pub use flui_platform::haptics::UnitRangeError;
    }
}

/// Gesture details, focus, the per-kind gesture presets, arena observation
/// (`GestureObserver`, which `flui-devtools`' gesture inspector builds on)
/// and the extension surface of a custom recognizer, at the paths
/// `flui::interaction` uses.
///
/// A third-party recognizer implements `GestureArenaMember` and
/// `GestureRecognizer` (whose `takes_down` answers a `DownTake` for the
/// Down its `add_pointer` would admit), joins the arena through `ArenaMembership` (with a
/// `MotionRole` when it moves) or `PrimaryContact`, composes with
/// `GestureComposition` or a `SequencedBranch`, admits device kinds through
/// `PointerKindSet`, and is mounted by `widgets::RawGestureDetector` through a
/// `widgets::GestureRecognizerFactory`. `AxisLock` and `TransformLock`
/// configure the drag and transform slots of `widgets::GestureDetector`, and
/// `SwipeGestureRecognizer` is the built-in a package's edge or fling gesture
/// composes with.
pub mod interaction {
    pub use flui_interaction::events::PointerEvent;
    pub use flui_interaction::events::PointerEventExt;
    pub use flui_interaction::focus::FocusNode;
    pub use flui_interaction::focus::FocusNodeChangeCallback;
    pub use flui_interaction::gesture::ArenaMembership;
    pub use flui_interaction::gesture::AxisLock;
    pub use flui_interaction::gesture::BeginContactError;
    pub use flui_interaction::gesture::CancelOutcome;
    pub use flui_interaction::gesture::CompositionBranch;
    pub use flui_interaction::gesture::CompositionError;
    pub use flui_interaction::gesture::CompositionKind;
    pub use flui_interaction::gesture::ContactEnd;
    pub use flui_interaction::gesture::ContactId;
    pub use flui_interaction::gesture::ContactSnapshot;
    pub use flui_interaction::gesture::ContestCancel;
    pub use flui_interaction::gesture::DownTake;
    pub use flui_interaction::gesture::DragDownDetails;
    pub use flui_interaction::gesture::DragEndDetails;
    pub use flui_interaction::gesture::GestureArena;
    pub use flui_interaction::gesture::GestureArenaEntry;
    pub use flui_interaction::gesture::GestureArenaMember;
    pub use flui_interaction::gesture::GestureBranches;
    pub use flui_interaction::gesture::GestureComposition;
    pub use flui_interaction::gesture::GestureContest;
    pub use flui_interaction::gesture::GestureDisposition;
    pub use flui_interaction::gesture::GestureEndReason;
    pub use flui_interaction::gesture::GestureObservation;
    pub use flui_interaction::gesture::GestureObserver;
    pub use flui_interaction::gesture::GestureProfiles;
    pub use flui_interaction::gesture::GestureRecognizer;
    pub use flui_interaction::gesture::GestureSettings;
    pub use flui_interaction::gesture::GestureSettingsProvider;
    pub use flui_interaction::gesture::MotionRole;
    pub use flui_interaction::gesture::MultiTapCount;
    pub use flui_interaction::gesture::MultiTapCountError;
    pub use flui_interaction::gesture::ObservedComposition;
    pub use flui_interaction::gesture::ObservedMember;
    pub use flui_interaction::gesture::ObserverHandle;
    pub use flui_interaction::gesture::PanClaim;
    pub use flui_interaction::gesture::PhysicalEdge;
    pub use flui_interaction::gesture::PrimaryContact;
    pub use flui_interaction::gesture::RejectionReason;
    pub use flui_interaction::gesture::SequencedBranch;
    pub use flui_interaction::gesture::SwipeDetails;
    pub use flui_interaction::gesture::SwipeGestureRecognizer;
    pub use flui_interaction::gesture::SwipeStart;
    pub use flui_interaction::gesture::TransformLock;
    pub use flui_interaction::gesture::cancel_all;
    #[cfg(feature = "unstable-input")]
    pub use flui_interaction::haptics::HapticObserver;
    #[cfg(feature = "unstable-input")]
    pub use flui_interaction::haptics::HapticsHandle;
    pub use flui_interaction::ids::PointerId;
    pub use flui_interaction::pointer::PointerDispatch;
    pub use flui_interaction::text_input::ClipboardBackend;
    pub use flui_interaction::text_input::ClipboardCapability;
    pub use flui_interaction::text_input::ClipboardHandle;
    pub use flui_interaction::text_input::PrimaryClaim;
    pub use flui_interaction::text_input::PrimaryClaimId;
    pub use flui_interaction::text_input::PrimaryClaims;
    pub use flui_interaction::text_input::PrimarySelectionBackend;
    pub use flui_interaction::text_input::PrimarySelectionCapability;
    pub use flui_interaction::text_input::PrimarySelectionHandle;
    pub use flui_interaction::text_input::SelectionText;
    pub use flui_platform::EventTime;
    pub use flui_platform::input::capabilities::PointerKindSet;
    pub use flui_platform::input::pointer::ButtonChange;
    pub use flui_platform::input::pointer::PointerKind;

    /// Drag-and-drop (ADR-0211): what a source widget starts through the
    /// presentation's `DragAndDrop` and what a receiver answers, the surface
    /// a package's reorderable list or drop zone is written against.
    pub mod drag {
        pub use flui_interaction::drag::AllowedActions;
        pub use flui_interaction::drag::Autoscroll;
        pub use flui_interaction::drag::DragAndDrop;
        pub use flui_interaction::drag::DragError;
        pub use flui_interaction::drag::DragIdentity;
        pub use flui_interaction::drag::DragItem;
        pub use flui_interaction::drag::DragObserver;
        pub use flui_interaction::drag::DragOutcome;
        pub use flui_interaction::drag::DragPoint;
        pub use flui_interaction::drag::DragReach;
        pub use flui_interaction::drag::DragSession;
        pub use flui_interaction::drag::DragStart;
        pub use flui_interaction::drag::DragTrigger;
        pub use flui_interaction::drag::DragView;
        pub use flui_interaction::drag::DropReceiver;
        pub use flui_interaction::drag::DropReceiverSlot;
        pub use flui_interaction::drag::MoveOutcome;
        pub use flui_interaction::drag::SpringLoading;
        pub use flui_platform::transfer::data::TransferActions;
    }

    /// What a text widget's store is built from (ADR-0151 §6): the lock
    /// arbiter that decides when a platform grant runs, the ledger of the
    /// committed text and the count of application edits.
    pub mod text_input {
        pub use flui_interaction::text_input::CompositionLedger;
        pub use flui_interaction::text_input::EditGeneration;
        pub use flui_interaction::text_input::LockArbiter;
        pub use flui_interaction::text_input::committed_text;
    }
}

/// Custom painting and the paint, style and text values, at the paths `flui::painting` uses.
///
/// `DrawOp` is here for packages' paint tests, which read back the recorded
/// operations; no package names it outside its tests.
pub mod painting {
    pub use flui_painting::Alignment;
    pub use flui_painting::AlignmentDirectional;
    pub use flui_painting::BlurSigma;
    pub use flui_painting::Canvas;
    pub use flui_painting::DrawOp;
    pub use flui_painting::TextSizing;
    pub use flui_painting::{paint, styling, typography};
    pub use flui_rendering::delegates::CustomPainter;
    pub use flui_semantics::SemanticsRegion;
    pub use flui_semantics::SemanticsRegionKey;
}

/// Render-object authoring, at the paths `flui::rendering` uses.
pub mod rendering {
    pub use flui_interaction::pointer::HitTestBehavior;
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::delegates::FlowDelegate;
    pub use flui_rendering::delegates::FlowPaintingContext;
    pub use flui_rendering::delegates::MultiChildLayoutContext;
    pub use flui_rendering::delegates::MultiChildLayoutDelegate;
    pub use flui_rendering::delegates::SingleChildLayoutDelegate;
    pub use flui_rendering::parent_data::TableCellVerticalAlignment;
    pub use flui_rendering::protocol::BoxProtocol;
    pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};

    /// The built-in render objects packages compose and the configuration types the widgets
    /// take from them, at `flui::rendering::objects`.
    pub mod objects {
        pub use flui_objects::CarouselLayout;
        pub use flui_objects::CrossAxisAlignment;
        pub use flui_objects::FloatingHeaderSnapConfiguration;
        pub use flui_objects::FocusRingStyle;
        pub use flui_objects::HitShape;
        pub use flui_objects::MainAxisAlignment;
        pub use flui_objects::MainAxisSize;
        pub use flui_objects::StackFit;
        pub use flui_objects::TableColumnWidth;
        pub use flui_objects::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction};
    }
}

/// The semantics vocabulary a package's widgets describe themselves with, at the paths
/// `flui::semantics` uses.
pub mod semantics {
    pub use flui_semantics::AutoComplete;
    pub use flui_semantics::CellPosition;
    pub use flui_semantics::Current;
    pub use flui_semantics::HasPopup;
    pub use flui_semantics::HeadingLevel;
    pub use flui_semantics::Invalid;
    pub use flui_semantics::LiveSetting;
    pub use flui_semantics::NumericRange;
    pub use flui_semantics::NumericRangeError;
    pub use flui_semantics::SemanticsAnchor;
    pub use flui_semantics::SemanticsRole;
    pub use flui_semantics::SetSize;
    pub use flui_semantics::SortDirection;
    pub use flui_semantics::TableShape;
    pub use flui_semantics::TextSelection;
    pub use flui_semantics::Toggled;
}

/// Render internals that the facade does not expose.
///
/// **Evolving:** these are internals of the render pipeline. They may
/// change or move on any train; an item graduates into a facade module once it
/// has stayed unchanged across trains and has a second consumer (ADR-0088 §4).
pub mod pipeline {
    pub use flui_rendering::view::ScrollDirection;
}

/// Development hooks the facade does not expose.
///
/// **Evolving** (ADR-0088 §4), like [`pipeline`]:
///
/// - frame telemetry that development tooling reads. No public API produces a
///   `FrameSnapshot` yet: the presentation's frame clock is internal to the
///   app host. A public snapshot source is the follow-up recorded in
///   ADR-0088's move 4.
/// - the plugin ABI of `flui-hot-reload`'s dlopen path (ADR-0311 §6): the
///   [`PluginPipeline`](hooks::PluginPipeline) a scene plugin mounts its root
///   on and the [`Scene`](hooks::Scene) of a retained version it hands back,
///   the [`TextContextHandle`](hooks::TextContextHandle) it measures with, and
///   the [`LayerTree`](hooks::LayerTree) a scene plugin builds its image from,
///   which `Scene::new` makes a version, and the [`SceneError`](hooks::SceneError)
///   a refused frame reports as its source. They leave with that path (ADR-0311 §7).
///
/// The tree-observation seam is not here; it is `foundation::observe`, at the
/// facade's path.
pub mod hooks {
    mod plugin_pipeline;

    pub use flui_layer::LayerTree;
    pub use flui_layer::Scene;
    pub use flui_layer::SceneError;
    pub use flui_rendering::pipeline::TextContextHandle;
    pub use flui_scheduler::FrameSnapshot;
    pub use plugin_pipeline::PluginFrameError;
    pub use plugin_pipeline::PluginPipeline;
}
