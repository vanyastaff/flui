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
//! - **Evolving module** [`pipeline`]: render-object internals the facade does
//!   not expose. Development-hook surface is added only with its real producer
//!   and consumer; there is no empty hooks module or scheduler snapshot bridge.
//! - **Opt-in package tests** [`testing`]: the facade's test-kit paths plus
//!   Evolving host, input and inspection helpers, absent from an ordinary graph.
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

/// Platform values: brightness, locale, a clipboard read's answer and a write, haptic
/// feedback kinds, the host's system preferences (its haptics setting among
/// them) and every type its accessors return, the target platform (the one a
/// gesture preset models), the window's state and mode, and the system
/// notices the host delivers.
pub mod platform {
    pub use flui_platform::AdmissionError;
    pub use flui_platform::Availability;
    pub use flui_platform::Brightness;
    pub use flui_platform::ClipboardError;
    pub use flui_platform::ClipboardRead;
    pub use flui_platform::ClipboardWrite;
    pub use flui_platform::Concealment;
    pub use flui_platform::DurationScale;
    pub use flui_platform::GesturePreferences;
    pub use flui_platform::HapticFeedback;
    pub use flui_platform::HostNotice;
    pub use flui_platform::HourCycle;
    pub use flui_platform::InvalidLocale;
    pub use flui_platform::InvalidPreference;
    pub use flui_platform::InvalidRegionalFormat;
    pub use flui_platform::Locale;
    pub use flui_platform::MeasurementSystem;
    pub use flui_platform::MemoryPressureLevel;
    pub use flui_platform::MotionPreference;
    pub use flui_platform::NativeMouseGeometry;
    pub use flui_platform::NativeTouchGeometry;
    pub use flui_platform::NumberingSystem;
    pub use flui_platform::RegionOverride;
    pub use flui_platform::RegionalFormats;
    pub use flui_platform::SessionEndBlock;
    pub use flui_platform::SessionEndRequest;
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
    /// Platform text-store vocabulary shared with text-editing widgets.
    pub mod text_store {
        pub use flui_platform::text_store::TextInputPurpose;
    }

    /// The capability seam (ADR-0152): what a package declares, provides and
    /// registers a platform capability with, and the owner-thread result of a
    /// call the operating system finishes later. A widget acquires a
    /// capability with `view::LifecycleContextExt::capability`.
    pub mod capability {
        pub use flui_platform::capability::Abandoned;
        pub use flui_platform::capability::Capability;
        pub use flui_platform::capability::CapabilityProvider;
        pub use flui_platform::capability::CapabilityRegistrar;
        pub use flui_platform::capability::Completion;
        pub use flui_platform::capability::CompletionSource;
        pub use flui_platform::capability::OwnerPoster;
        pub use flui_platform::capability::OwnerRetired;
        pub use flui_platform::capability::Plugin;
        pub use flui_platform::capability::ProviderContext;
        pub use flui_platform::capability::Resolver;
        pub use flui_platform::capability::WindowRef;
    }

    /// Handle-less input capabilities (ADR-0213). Unstable: present only
    /// under the additive `unstable-input` feature.
    #[cfg(feature = "unstable-input")]
    pub mod input_capabilities {
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
    pub use flui_interaction::ClipboardBackend;
    pub use flui_interaction::ClipboardCapability;
    pub use flui_interaction::ClipboardHandle;
    pub use flui_interaction::ContactEnd;
    pub use flui_interaction::ContactId;
    pub use flui_interaction::ContactSnapshot;
    pub use flui_interaction::DragDownDetails;
    pub use flui_interaction::DragEndDetails;
    pub use flui_interaction::DragStartDetails;
    pub use flui_interaction::DragUpdateDetails;
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
    pub use flui_interaction::MultiTapCount;
    pub use flui_interaction::MultiTapCountError;
    pub use flui_interaction::ObserverHandle;
    pub use flui_interaction::PanZoomEvent;
    pub use flui_interaction::PhysicalEdge;
    pub use flui_interaction::PointerDispatch;
    pub use flui_interaction::PointerEvent;
    pub use flui_interaction::PointerEventExt;
    pub use flui_interaction::PointerId;
    pub use flui_interaction::PointerKind;
    pub use flui_interaction::PointerKindSet;
    pub use flui_interaction::PrimaryClaim;
    pub use flui_interaction::PrimaryClaimId;
    pub use flui_interaction::PrimaryClaims;
    pub use flui_interaction::PrimaryContact;
    pub use flui_interaction::PrimarySelectionBackend;
    pub use flui_interaction::PrimarySelectionCapability;
    pub use flui_interaction::PrimarySelectionHandle;
    pub use flui_interaction::SelectionText;
    pub use flui_interaction::SwipeDetails;
    pub use flui_interaction::SwipeGestureRecognizer;
    pub use flui_interaction::SwipeStart;
    pub use flui_interaction::TapDetails;
    pub use flui_interaction::TransformLock;
    pub use flui_interaction::cancel_all;
    pub use flui_interaction::events::ButtonChange;
    pub use flui_interaction::events::Code;
    pub use flui_interaction::events::CursorIcon;
    pub use flui_interaction::events::Key;
    pub use flui_interaction::events::KeyEvent;
    pub use flui_interaction::events::KeyState;
    pub use flui_interaction::events::NamedKey;
    pub use flui_interaction::events::PointerButtons;
    pub use flui_interaction::focus::FocusNode;
    pub use flui_interaction::focus::FocusNodeChangeCallback;
    pub use flui_interaction::focus::KeyEventResult;
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
    pub use flui_interaction::ids::DeviceId;
    pub use flui_interaction::pointer::EventPropagation;
    pub use flui_interaction::pointer::HitTestBehavior;
    pub use flui_platform::EventTime;
    pub use flui_platform::pointer::PointerButton;
    pub use flui_platform::pointer::PointerPress;
    pub use flui_platform::pointer::PointerRelease;

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
    pub use flui_painting::BlurSigma;
    pub use flui_painting::Canvas;
    pub use flui_painting::DrawOp;
    pub use flui_painting::ImagePlacement;
    pub use flui_painting::TextSizing;
    pub use flui_painting::paint::Paint;
    pub use flui_painting::paint::StrokeCap;
    pub use flui_rendering::delegates::CustomPainter;
    pub use flui_rendering::semantics::SemanticsRegion;
    pub use flui_rendering::semantics::SemanticsRegionKey;

    /// Paths, clips and shape morphing, at the facade's path.
    pub mod paint {
        pub use flui_painting::paint::Clip;
        pub use flui_painting::paint::ClipOp;
        pub use flui_painting::paint::ImageRepeat;
        pub use flui_painting::paint::Morph;
        pub use flui_painting::paint::Path;
        pub use flui_painting::paint::PathCommand;
        pub use flui_painting::paint::RoundedPolygon;
        pub use flui_painting::paint::shapes::material_shapes;
    }

    /// Colours, borders, decorations and shadows, at the facade's path.
    pub mod styling {
        pub use flui_painting::styling::Border;
        pub use flui_painting::styling::BorderRadius;
        pub use flui_painting::styling::BorderRadiusExt;
        pub use flui_painting::styling::BorderSide;
        pub use flui_painting::styling::BorderStyle;
        pub use flui_painting::styling::BoxDecoration;
        pub use flui_painting::styling::BoxShadow;
        pub use flui_painting::styling::Color;
        pub use flui_painting::styling::Shadow;
        pub use flui_painting::styling::TableBorder;
    }

    /// Text styles and decoration vocabulary, at the facade's path.
    pub mod typography {
        pub use flui_painting::typography::DecorationThickness;
        pub use flui_painting::typography::FontWeight;
        pub use flui_painting::typography::TextDecoration;
        pub use flui_painting::typography::TextDecorationLines;
        pub use flui_painting::typography::TextDecorationStyle;
        pub use flui_painting::typography::TextDirection;
        pub use flui_painting::typography::TextSpan;
        pub use flui_painting::typography::TextStyle;
    }
}

/// Render-object and semantics authoring, at the paths `flui::rendering` uses.
pub mod rendering {
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::delegates::AspectRatioDelegate;
    pub use flui_rendering::delegates::CenterLayoutDelegate;
    pub use flui_rendering::delegates::FlowDelegate;
    pub use flui_rendering::delegates::FlowPaintingContext;
    pub use flui_rendering::delegates::MultiChildLayoutContext;
    pub use flui_rendering::delegates::MultiChildLayoutDelegate;
    pub use flui_rendering::delegates::SingleChildLayoutDelegate;
    pub use flui_rendering::delegates::SliverGridDelegate;
    pub use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
    pub use flui_rendering::delegates::SliverGridDelegateWithMaxCrossAxisExtent;
    pub use flui_rendering::delegates::SliverGridLayout;
    pub use flui_rendering::parent_data::{FlexFit, TableCellVerticalAlignment};
    pub use flui_rendering::protocol::BoxProtocol;
    pub use flui_rendering::semantics::ActionCall;
    pub use flui_rendering::semantics::AnnounceError;
    pub use flui_rendering::semantics::Announcer;
    pub use flui_rendering::semantics::AutoComplete;
    pub use flui_rendering::semantics::CellPosition;
    pub use flui_rendering::semantics::Current;
    pub use flui_rendering::semantics::HasPopup;
    pub use flui_rendering::semantics::HeadingLevel;
    pub use flui_rendering::semantics::Invalid;
    pub use flui_rendering::semantics::LiveSetting;
    pub use flui_rendering::semantics::NumericRange;
    pub use flui_rendering::semantics::NumericRangeError;
    pub use flui_rendering::semantics::Reveal;
    pub use flui_rendering::semantics::ScrollUnit;
    pub use flui_rendering::semantics::SemanticsAnchor;
    pub use flui_rendering::semantics::SemanticsConfiguration;
    pub use flui_rendering::semantics::SemanticsRole;
    pub use flui_rendering::semantics::SetSize;
    pub use flui_rendering::semantics::SortDirection;
    pub use flui_rendering::semantics::TableShape;
    pub use flui_rendering::semantics::TextDirection;
    pub use flui_rendering::semantics::TextSelection;
    pub use flui_rendering::semantics::Toggled;
    pub use flui_rendering::semantics::Urgency;
    pub use flui_rendering::view::ScrollPosition;
    pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};
}

/// Render objects and their configuration that the facade does not expose.
///
/// **Evolving:** these are internals of the render-object catalog. They may
/// change or move on any train; an item graduates into a facade module once it
/// has stayed unchanged across trains and has a second consumer (ADR-0088 §4).
pub mod pipeline {
    pub use flui_objects::CarouselLayout;
    pub use flui_objects::CarouselLayoutError;
    pub use flui_objects::CarouselLayoutKind;
    pub use flui_objects::CrossAxisAlignment;
    pub use flui_objects::FloatingHeaderSnapConfiguration;
    pub use flui_objects::FocusRingStyle;
    pub use flui_objects::HitShape;
    pub use flui_objects::InvalidSliverCrossAxisExtent;
    pub use flui_objects::MAX_ITEM_EXTENT;
    pub use flui_objects::MainAxisAlignment;
    pub use flui_objects::MainAxisSize;
    pub use flui_objects::Oval;
    pub use flui_objects::OverScrollHeaderStretchConfiguration;
    pub use flui_objects::OverflowBoxFit;
    pub use flui_objects::PathClipConfiguration;
    pub use flui_objects::RenderPhysicalShape;
    pub use flui_objects::ShimmerBand;
    pub use flui_objects::StackFit;
    pub use flui_objects::StretchTriggerSignal;
    pub use flui_objects::TableColumnWidth;
    pub use flui_objects::TranslationFraction;
    pub use flui_objects::WrapAlignment;
    pub use flui_objects::WrapCrossAlignment;
    pub use flui_rendering::view::ScrollDirection;
}

/// Deterministic package tests, available only with the `testing` feature.
///
/// The common surface mirrors `flui::testing` at the same paths. The additional
/// host, input and inspection helpers are Evolving SDK surface for packages
/// whose tests need more than the Stable facade exposes.
#[cfg(feature = "testing")]
pub mod testing {
    pub use flui_testing::widgets;
    pub use flui_testing::{
        AlternativesSubject, BuildCapabilities, GestureAlternativesCheck, HeadlessBinding,
        HeadlessHost, HeadlessWindow, MountOptions, MountOwners, Mounted, PointerScript,
        TouchScript, assert_gesture_alternatives,
    };
    pub use flui_testing::{DropLedger, DropSentinel};
    pub use flui_testing::{a11y, log_capture, replay, storage};

    /// Synthetic keyboard and pointer input for package tests.
    pub mod input {
        pub use flui_interaction::events::make_down_event_for_id;
        pub use flui_interaction::events::make_down_event_for_id_with_button;
        pub use flui_interaction::events::make_move_event_for_id;
        pub use flui_interaction::gesture::GestureSettingsSource;
        pub use flui_interaction::testing::input::KeyEventBuilder;
    }

    /// Text measurement resources a package's painting tests inspect.
    pub mod painting {
        pub use flui_painting::glyphs::FontRegistry;
    }

    /// Render-object tests and committed layer-tree inspection.
    pub mod rendering {
        pub use flui_rendering::layer::{Layer, LayerTree};
        pub use flui_rendering::testing::inspect::render_snapshot;
        pub use flui_rendering::testing::{
            BoxQueryRun, DrawCommandSummary, DrawKind, FrameRun, LayoutRun, PaintRun, Probe,
            RenderTester, TreeNode, box_node, collect_commands, sliver_node,
        };
    }
}
