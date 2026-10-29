//! The package-author surface of FLUI (ADR-0088 §4).
//!
//! A package such as a design system depends on this crate instead of on the
//! internal crates one by one, and it never pulls in the host, the engine or
//! the GPU stack: `flui-app`, `flui-engine` and `wgpu` are outside its normal
//! dependency closure, which `cargo xtask reach` checks.
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
//!   [`platform`] and [`rendering`] hold the subset of the facade's module that
//!   packages use, as the same items, not wrappers.
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

pub use flui_animation as animation;
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

/// Platform values: brightness, locale, haptic feedback kinds, the host's
/// system preferences (its haptics setting among them) and every type its
/// accessors return, the target platform (the one a gesture preset models),
/// the window's state and mode, and the system notices the host delivers.
pub mod platform {
    pub use flui_platform::AdmissionError;
    pub use flui_platform::Brightness;
    pub use flui_platform::DurationScale;
    pub use flui_platform::GesturePreferences;
    pub use flui_platform::HapticFeedback;
    pub use flui_platform::HostNotice;
    pub use flui_platform::InvalidLocale;
    pub use flui_platform::InvalidPreference;
    pub use flui_platform::Locale;
    pub use flui_platform::MemoryPressureLevel;
    pub use flui_platform::MotionPreference;
    pub use flui_platform::NativeMouseGeometry;
    pub use flui_platform::NativeTouchGeometry;
    pub use flui_platform::SessionEndBlock;
    pub use flui_platform::SessionEndRequest;
    pub use flui_platform::SubmitError;
    pub use flui_platform::SystemPreferences;
    pub use flui_platform::TargetPlatform;
    pub use flui_platform::TextWeightPreference;
    pub use flui_platform::Unsupported;
    pub use flui_platform::UnsupportedReason;
    pub use flui_platform::WheelPreferences;
    pub use flui_platform::WheelStep;
    pub use flui_platform::WindowMode;
    pub use flui_platform::WindowState;
    pub use flui_platform::text_store::TextInputPurpose;

    /// Handle-less input capabilities (ADR-0213). Unstable: present only
    /// under the additive `unstable-input` feature.
    #[cfg(feature = "unstable-input")]
    pub mod input_capabilities {
        pub use flui_platform::Availability;
        pub use flui_platform::input_capabilities::InputCapabilities;
        pub use flui_platform::input_capabilities::PointerKindSet;
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
/// `GestureRecognizer`, joins the arena through `ArenaMembership` (with a
/// `MotionRole` when it moves) or `PrimaryContact`, composes with
/// `GestureComposition` or a `SequencedBranch`, admits device kinds through
/// `PointerKindSet`, and is mounted by `widgets::RawGestureDetector` through a
/// `widgets::GestureRecognizerFactory`. `AxisLock` and `TransformLock`
/// configure the drag and transform slots of `widgets::GestureDetector`, and
/// `SwipeGestureRecognizer` is the built-in a package's edge or fling gesture
/// composes with.
pub mod interaction {
    pub use flui_interaction::ArenaMembership;
    pub use flui_interaction::AxisLock;
    pub use flui_interaction::BeginContactError;
    pub use flui_interaction::CancelOutcome;
    pub use flui_interaction::ContactEnd;
    pub use flui_interaction::ContactId;
    pub use flui_interaction::ContactSnapshot;
    pub use flui_interaction::DragDownDetails;
    pub use flui_interaction::GestureArena;
    pub use flui_interaction::GestureArenaEntry;
    pub use flui_interaction::GestureArenaMember;
    pub use flui_interaction::GestureDisposition;
    pub use flui_interaction::GestureEndReason;
    pub use flui_interaction::GestureObservation;
    pub use flui_interaction::GestureObserver;
    pub use flui_interaction::GestureProfiles;
    pub use flui_interaction::GestureRecognizer;
    pub use flui_interaction::GestureSettings;
    pub use flui_interaction::GestureSettingsProvider;
    #[cfg(feature = "unstable-input")]
    pub use flui_interaction::HapticObserver;
    #[cfg(feature = "unstable-input")]
    pub use flui_interaction::HapticsHandle;
    pub use flui_interaction::ObserverHandle;
    pub use flui_interaction::PhysicalEdge;
    pub use flui_interaction::PointerDispatch;
    pub use flui_interaction::PointerEvent;
    pub use flui_interaction::PointerEventExt;
    pub use flui_interaction::PointerId;
    pub use flui_interaction::PointerKind;
    pub use flui_interaction::PointerKindSet;
    pub use flui_interaction::PrimaryContact;
    pub use flui_interaction::SwipeDetails;
    pub use flui_interaction::SwipeGestureRecognizer;
    pub use flui_interaction::SwipeStart;
    pub use flui_interaction::TransformLock;
    pub use flui_interaction::cancel_all;
    pub use flui_interaction::focus::FocusNode;
    pub use flui_interaction::focus::FocusSubscription;
    pub use flui_interaction::gesture::CompositionBranch;
    pub use flui_interaction::gesture::CompositionError;
    pub use flui_interaction::gesture::CompositionKind;
    pub use flui_interaction::gesture::ContestCancel;
    pub use flui_interaction::gesture::GestureBranches;
    pub use flui_interaction::gesture::GestureComposition;
    pub use flui_interaction::gesture::GestureContest;
    pub use flui_interaction::gesture::MotionRole;
    pub use flui_interaction::gesture::ObservedComposition;
    pub use flui_interaction::gesture::ObservedMember;
    pub use flui_interaction::gesture::PanClaim;
    pub use flui_interaction::gesture::RejectionReason;
    pub use flui_interaction::gesture::SequencedBranch;
    pub use flui_platform::EventTime;

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
        pub use flui_platform::data_transfer::TransferActions;
    }
}

/// Custom painting and the paint, style and text values, at the paths `flui::painting` uses.
///
/// `DrawOp` is here for packages' paint tests, which read back the recorded
/// operations; no package names it outside its tests.
pub mod painting {
    pub use flui_painting::Alignment;
    pub use flui_painting::Canvas;
    pub use flui_painting::DrawOp;
    pub use flui_painting::TextSizing;
    pub use flui_painting::paint::Clip;
    pub use flui_painting::paint::ClipOp;
    pub use flui_painting::paint::Morph;
    pub use flui_painting::paint::Paint;
    pub use flui_painting::paint::Path;
    pub use flui_painting::paint::PathCommand;
    pub use flui_painting::paint::RoundedPolygon;
    pub use flui_painting::paint::StrokeCap;
    pub use flui_painting::paint::shapes::material_shapes;
    pub use flui_painting::styling::Border;
    pub use flui_painting::styling::BorderRadius;
    pub use flui_painting::styling::BorderRadiusExt;
    pub use flui_painting::styling::BorderSide;
    pub use flui_painting::styling::BorderStyle;
    pub use flui_painting::styling::BoxDecoration;
    pub use flui_painting::styling::BoxShadow;
    pub use flui_painting::styling::Color;
    pub use flui_painting::typography::FontWeight;
    pub use flui_painting::typography::TextDirection;
    pub use flui_painting::typography::TextStyle;
    pub use flui_rendering::semantics::SemanticsRegion;
    pub use flui_rendering::semantics::SemanticsRegionKey;
}

/// Render-object authoring, at the paths `flui::rendering` uses.
pub mod rendering {
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::hit_testing::HitTestBehavior;
    pub use flui_rendering::protocol::BoxProtocol;
    pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};
}

/// Render objects and their configuration that the facade does not expose.
///
/// **Evolving:** these are internals of the render-object catalog. They may
/// change or move on any train; an item graduates into a facade module once it
/// has stayed unchanged across trains and has a second consumer (ADR-0088 §4).
pub mod pipeline {
    pub use flui_objects::PathClipConfiguration;
    pub use flui_objects::RenderPhysicalShape;
    pub use flui_objects::TranslationFraction;
    pub use flui_rendering::view::ScrollDirection;
}

/// Development hooks the facade does not expose.
///
/// **Evolving** (ADR-0088 §4), like [`pipeline`]: frame telemetry that
/// development tooling reads. The tree-observation seam is not here; it is
/// `foundation::observe`, at the facade's path.
///
/// No public API produces a `FrameSnapshot` yet: the
/// presentation's frame clock is internal to the app host. A public snapshot
/// source is the follow-up recorded in ADR-0088's move 4.
pub mod hooks {
    pub use flui_scheduler::FrameSnapshot;
}
