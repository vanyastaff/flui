//! The SDK's surface is the measured list, and each re-export is the item it
//! names, not a copy.
//!
//! The list starts with the package imports measured in ARCHITECTURE.md and
//! pins subsequent consumer additions and removals, plus `painting::DrawOp`,
//! which the design systems' paint tests read, `view::dev_reload`, the
//! hook `flui-hot-reload`'s host drivers implement (ADR-0094 §1), and
//! `view::dev_agent`, the hook `flui-devtools`' agent server implements
//! (ADR-0095 §3), and `view::FrameWaker`, the one scheduler capability a
//! package hands a worker thread (ADR-0136 §2). The input-completeness surface
//! the spec places in the SDK follows: arena observation, the per-kind gesture
//! presets, system preferences and haptic feedback kinds, and input regions.
//! Animation prelude imports also pin the shared package-author entry point.
//! `custom_recognizer` proves the extension surface with a recognizer written
//! against the SDK alone.

#[path = "surface/custom_recognizer.rs"]
mod custom_recognizer;

/// Every measured item through its SDK path: removing or moving one fails to
/// build this test.
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod measured {
    use flui_sdk::animation::prelude::{
        Animatable as _, AnimatedValue as _, Animation as _, AnimationController as _,
        AnimationStatus as _, ArcCurve as _, Curve as _, DrivenController as _, Keyframes as _,
        Listenable as _, MotionSpec as _, Tween as _, TwoWayConverter as _,
    };
    use flui_sdk::animation::{
        Animatable as _, AnimatedValue as _, AnimatedValueView as _, Animation as _,
        AnimationBehavior as _, AnimationController as _, AnimationError as _, AnimationInput as _,
        AnimationRunFuture as _, AnimationStatus as _, AnimationVector as _, ArcCurve as _,
        ConstantAnimation as _, Cubic as _, Curve as _, CurvedAnimation as _,
        DrivenController as _, JumpAt as _, Keyframes as _, MotionPolicy as _, MotionSpec as _,
        MotionUpdate as _, PlaybackRate as _, RangeDefect as _, RepeatMode as _, RunCanceled as _,
        Stagger as _, StaggerOrigin as _, StatusSubscription as _, Steps as _, Tween as _,
        TwoWayConverter as _, ValueRange as _, Vsync as _, VsyncPublication as _,
        VsyncRegistration as _, VsyncRegistrationError as _, VsyncRetirement as _,
        VsyncUpdate as _, curves as _,
    };
    use flui_sdk::foundation::notifier::Listenable as _;
    use flui_sdk::foundation::observe::{
        ElementMounted as _, ElementMoved as _, ElementRebuilt as _, ElementUnmounted as _,
        TreeObserver as _,
    };
    use flui_sdk::foundation::panic::RecoveryScope as _;
    use flui_sdk::foundation::{
        ChangeNotifier as _, ElementId as _, Listenable as _, ListenerCallback as _,
        ListenerId as _, ListenerObserver as _, RebuildReason as _, TextScaleProfile as _,
        TextSizingIntent as _, ViewKey as _,
    };
    use flui_sdk::geometry::{EdgeInsets as _, RRect as _, Radius as _};
    use flui_sdk::hooks::FrameSnapshot as _;
    use flui_sdk::interaction::{
        ArenaMembership as _, BeginContactError as _, CancelOutcome as _, CompositionError as _,
        CompositionKind as _, ContactId as _, ContactSnapshot as _, ContestCancel as _,
        DragDownDetails as _, EventTime as _, FocusNode as _, FocusSubscription as _,
        GestureArena as _, GestureArenaEntry as _, GestureArenaMember as _, GestureBranches as _,
        GestureComposition as _, GestureContest as _, GestureDisposition as _,
        GestureEndReason as _, GestureObservation as _, GestureObserver as _, GestureProfiles as _,
        GestureRecognizer as _, GestureSettings as _, GestureSettingsProvider as _,
        MotionRole as _, ObservedMember as _, ObserverHandle as _, PanClaim as _,
        PointerDispatch as _, PointerEvent as _, PointerEventExt as _, PointerId as _,
        PointerKind as _, PointerKindSet as _, PrimaryContact as _, SequencedBranch as _,
        cancel_all as _,
    };
    use flui_sdk::painting::Alignment as _;
    use flui_sdk::painting::{
        Border as _, BorderRadius as _, BorderRadiusExt as _, BorderSide as _, BorderStyle as _,
        BoxDecoration as _, Color as _,
    };
    use flui_sdk::painting::{Canvas as _, DrawOp as _};
    use flui_sdk::painting::{Clip as _, Paint as _, Path as _};
    use flui_sdk::painting::{
        FontWeight as _, TextDirection as _, TextSizing as _, TextStyle as _,
    };
    use flui_sdk::pipeline::{
        PathClipConfiguration as _, RenderPhysicalShape as _, TranslationFraction as _,
    };
    use flui_sdk::platform::{
        Brightness as _, HapticFeedback as _, InvalidLocale as _, Locale as _,
        SystemPreferences as _, TargetPlatform as _,
    };
    use flui_sdk::rendering::{
        BoxConstraints as _, BoxProtocol as _, HitTestBehavior as _, InputRegion as _,
        InputRegionKind as _, InputRegionSink as _, RenderError as _, RenderResult as _,
        RenderUpdateImpact as _,
    };
    use flui_sdk::view::dev_agent::{
        AgentAnswer as _, AgentFault as _, AgentWindow as _, DevAgentHook as _, HandleKind as _,
    };
    use flui_sdk::view::dev_reload::{DevReloadHook as _, ReloadEvent as _, ReloadWake as _};
    use flui_sdk::view::element::ElementKind as _;
    use flui_sdk::view::prelude::{BuildContext as _, InheritedData as _, StatelessView as _};
    use flui_sdk::view::{
        AnimatedView as _, BoxedView as _, BuildContext as _, BuildContextExt as _, Child as _,
        FieldMask as _, FrameWaker as _, GlobalKey as _, InheritedData as _, InheritedView as _,
        IntoView as _, PostFrameHandle as _, RebuildHandle as _, RebuildReason as _,
        RenderObjectContext as _, RenderView as _, StatefulView as _, View as _, ViewExt as _,
        ViewState as _, impl_animated_view as _, impl_inherited_view as _, impl_render_view as _,
        single_child_view_children as _,
    };
    use flui_sdk::widgets::__private::{TextFormFieldConfig as _, TextFormFieldCore as _};
    use flui_sdk::widgets::animated::VsyncScope as _;
    use flui_sdk::widgets::icon::IconData as _;
    use flui_sdk::widgets::layout::PreferredSizeView as _;
    use flui_sdk::widgets::prelude::BoxConstraints as _;
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, Align as _, AnimatedBuilder as _, AppBuilder as _,
        Assertiveness as _, AutovalidateMode as _, BoxedLocalizationsDelegate as _,
        ButtonActivateIntent as _, CallbackAction as _, Center as _, ClipRect as _,
        ColoredBox as _, Column as _, ConstrainedBox as _, Container as _, ContentShape as _,
        CrossAxisAlignment as _, CustomMultiChildLayout as _, CustomPaint as _, CustomPainter as _,
        DecoratedBox as _, DefaultTextStyle as _, Directionality as _, Disclosure as _,
        DisclosureState as _, EditableText as _, Expanded as _, ExpansionState as _,
        FadeTransition as _, Flexible as _, FloatingHeaderSnapConfiguration as _, Focus as _,
        FormFieldHandle as _, GestureArenaScope as _, GestureDetector as _,
        GestureRecognizerFactory as _, HeadingLevel as _, HeroMode as _, HitShape as _,
        HitSlop as _, HitTestBehavior as _, Icon as _, IconData as _, IconTheme as _,
        IconThemeData as _, InheritedTheme as _, IntrinsicWidth as _, LayoutId as _,
        MainAxisAlignment as _, MainAxisSize as _, MediaQuery as _, MediaQueryData as _,
        MergeSemantics as _, MouseRegion as _, MultiChildLayoutContext as _,
        MultiChildLayoutDelegate as _, NavigatorHandle as _, NavigatorObserver as _,
        NumericRange as _, NumericRangeError as _, Offstage as _, Opacity as _, Padding as _,
        PageRoute as _, PanClaimRegion as _, PopupRoute as _, Positioned as _,
        PreferredSizeView as _, RawGestureDetector as _, RecognizerContext as _,
        RouteAnimation as _, RouteResult as _, Row as _, SafeArea as _, SelectAllTextIntent as _,
        Semantics as _, SemanticsAnchor as _, SemanticsRole as _, SizedBox as _,
        SlideTransition as _, Slider as _, SliderState as _, SliverPersistentHeader as _,
        SliverPersistentHeaderDelegate as _, Stack as _, StackFit as _, SubmitCallback as _,
        Table as _, TableCell as _, TableCellVerticalAlignment as _, TableColumnWidth as _,
        TableRow as _, Text as _, TextEditingController as _, TickerMode as _, Toggled as _,
        Transform as _, WidgetState as _, WidgetStateConstraint as _, WidgetStateProperty as _,
        WidgetStates as _, WidgetStatesController as _, WidgetsApp as _,
    };
    use flui_sdk::{
        geometry::{EdgeInsets as _, Offset as _, Point as _, RRect as _, Rect as _, Size as _},
        painting::{Alignment as _, Color as _},
    };
}

/// Each curated item, and one type per whole-module re-export, is the facade's
/// own type: this fails to build if an SDK path becomes a wrapper, a newtype
/// or a different item of the same name.
#[test]
fn the_re_exports_are_the_facades_types() {
    let _: fn(flui::animation::AnimationController) -> flui_sdk::animation::AnimationController =
        |x| x;
    let _: fn(flui::animation::DrivenController) -> flui_sdk::animation::prelude::DrivenController =
        |x| x;
    let _: fn(
        flui::animation::AnimatedValue<f64>,
    ) -> flui_sdk::animation::prelude::AnimatedValue<f64> = |x| x;
    let _: fn(
        flui::animation::AnimatedValueView<f64>,
    ) -> flui_sdk::animation::prelude::AnimatedValueView<f64> = |x| x;
    let _: fn(
        flui::animation::AnimationRunFuture,
    ) -> flui_sdk::animation::prelude::AnimationRunFuture = |x| x;
    let _: fn(flui::animation::MotionSpec) -> flui_sdk::animation::prelude::MotionSpec = |x| x;
    let _: fn(flui::animation::Keyframes<f64>) -> flui_sdk::animation::prelude::Keyframes<f64> =
        |x| x;
    let _: fn(flui::foundation::ElementId) -> flui_sdk::foundation::ElementId = |x| x;
    let _: fn(flui::geometry::Rect) -> flui_sdk::geometry::Rect = |x| x;
    let _: fn(flui::painting::styling::Color) -> flui_sdk::painting::Color = |x| x;
    let _: fn(flui::view::RebuildHandle) -> flui_sdk::view::RebuildHandle = |x| x;
    let _: fn(flui::view::WidgetsBinding) -> flui_sdk::view::WidgetsBinding = |x| x;
    let _: fn(flui::view::dev_reload::ReloadWake) -> flui_sdk::view::dev_reload::ReloadWake = |x| x;
    let _: fn(flui::view::persist::SaveStatus) -> flui_sdk::view::persist::SaveStatus = |x| x;
    let _: fn(flui::widgets::Text) -> flui_sdk::widgets::Text = |x| x;

    let _: fn(flui::interaction::GestureEndReason) -> flui_sdk::interaction::GestureEndReason =
        |x| x;
    let _: fn(flui::interaction::DragDownDetails) -> flui_sdk::interaction::DragDownDetails = |x| x;
    let _: fn(flui::interaction::FocusNode) -> flui_sdk::interaction::FocusNode = |x| x;
    let _: fn(flui::interaction::FocusSubscription) -> flui_sdk::interaction::FocusSubscription =
        |x| x;
    let _: fn(flui::interaction::GestureProfiles) -> flui_sdk::interaction::GestureProfiles = |x| x;
    let _: fn(flui::interaction::GestureSettings) -> flui_sdk::interaction::GestureSettings = |x| x;
    let _: fn(flui::interaction::GestureArena) -> flui_sdk::interaction::GestureArena = |x| x;
    let _: fn(flui::interaction::PrimaryContact) -> flui_sdk::interaction::PrimaryContact = |x| x;
    let _: fn(flui::interaction::GestureComposition) -> flui_sdk::interaction::GestureComposition =
        |x| x;
    let _: fn(flui::interaction::SequencedBranch) -> flui_sdk::interaction::SequencedBranch = |x| x;
    let _: fn(flui::interaction::MotionRole) -> flui_sdk::interaction::MotionRole = |x| x;
    let _: fn(flui::interaction::PanClaim) -> flui_sdk::interaction::PanClaim = |x| x;
    let _: fn(flui::interaction::PointerKindSet) -> flui_sdk::interaction::PointerKindSet = |x| x;
    let _: fn(
        flui::interaction::GestureSettingsProvider,
    ) -> flui_sdk::interaction::GestureSettingsProvider = |x| x;
    let _: fn(
        Box<dyn flui::interaction::GestureObserver>,
    ) -> Box<dyn flui_sdk::interaction::GestureObserver> = |x| x;
    let _: fn(flui::interaction::EventTime) -> flui_sdk::interaction::EventTime = |x| x;
    let _: fn(
        Box<dyn flui::interaction::GestureRecognizer>,
    ) -> Box<dyn flui_sdk::interaction::GestureRecognizer> = |x| x;
    let _: fn(flui::widgets::RecognizerContext) -> flui_sdk::widgets::RecognizerContext = |x| x;
    let _: fn(flui::platform::SystemPreferences) -> flui_sdk::platform::SystemPreferences = |x| x;
    let _: fn(flui::platform::TargetPlatform) -> flui_sdk::platform::TargetPlatform = |x| x;
    let _: fn(flui::platform::HapticFeedback) -> flui_sdk::platform::HapticFeedback = |x| x;
    let _: fn(flui::painting::Canvas) -> flui_sdk::painting::Canvas = |x| x;
    let _: fn(flui::painting::DrawOp) -> flui_sdk::painting::DrawOp = |x| x;
    let _: fn(flui::painting::TextSizing) -> flui_sdk::painting::TextSizing = |x| x;
    let _: fn(flui::platform::Brightness) -> flui_sdk::platform::Brightness = |x| x;
    let _: fn(flui::platform::InvalidLocale) -> flui_sdk::platform::InvalidLocale = |x| x;
    let _: fn(flui::rendering::RenderUpdateImpact) -> flui_sdk::rendering::RenderUpdateImpact =
        |x| x;
    let _: fn(flui::rendering::RenderError) -> flui_sdk::rendering::RenderError = |x| x;
    let _: fn(flui::rendering::RenderResult<()>) -> flui_sdk::rendering::RenderResult<()> = |x| x;
    let _: fn(flui::rendering::BoxConstraints) -> flui_sdk::rendering::BoxConstraints = |x| x;
    let _: fn(flui::rendering::HitTestBehavior) -> flui_sdk::rendering::HitTestBehavior = |x| x;
    let _: fn(flui::rendering::BoxProtocol) -> flui_sdk::rendering::BoxProtocol = |x| x;
}

/// Every line of `src/lib.rs` that declares public surface, trimmed and sorted.
///
/// Any `pub` item counts, not only `pub use` and `pub mod`, and so does
/// `#[macro_export]`. Because the list compares whole lines, a re-export must
/// name one item per line: a brace group spread over lines shows up as its
/// `pub use x::{` line, which is not in the list.
fn declared_surface() -> Vec<&'static str> {
    let mut lines: Vec<&str> = include_str!("../src/lib.rs")
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") || line.starts_with("#[macro_export]"))
        .collect();
    lines.sort_unstable();
    lines
}

/// Adding an item to the SDK is a decision (ADR-0088 §4 caps the Evolving
/// surface), so it changes this list in the same change.
#[test]
fn the_public_surface_is_the_measured_list() {
    let pinned = [
        "pub mod hooks {",
        "pub mod input_capabilities {",
        "pub mod interaction {",
        "pub mod painting {",
        "pub mod pipeline {",
        "pub mod platform {",
        "pub mod rendering {",
        "pub mod view {",
        "pub use flui_animation as animation;",
        "pub use flui_foundation as foundation;",
        "pub use flui_foundation::geometry;",
        "pub use flui_interaction::ArenaMembership;",
        "pub use flui_interaction::BeginContactError;",
        "pub use flui_interaction::CancelOutcome;",
        "pub use flui_interaction::ContactId;",
        "pub use flui_interaction::ContactSnapshot;",
        "pub use flui_interaction::DragDownDetails;",
        "pub use flui_interaction::GestureArena;",
        "pub use flui_interaction::GestureArenaEntry;",
        "pub use flui_interaction::GestureArenaMember;",
        "pub use flui_interaction::GestureDisposition;",
        "pub use flui_interaction::GestureEndReason;",
        "pub use flui_interaction::GestureObservation;",
        "pub use flui_interaction::GestureObserver;",
        "pub use flui_interaction::GestureProfiles;",
        "pub use flui_interaction::GestureRecognizer;",
        "pub use flui_interaction::GestureSettings;",
        "pub use flui_interaction::GestureSettingsProvider;",
        "pub use flui_interaction::ObserverHandle;",
        "pub use flui_interaction::PointerDispatch;",
        "pub use flui_interaction::PointerEvent;",
        "pub use flui_interaction::PointerEventExt;",
        "pub use flui_interaction::PointerId;",
        "pub use flui_interaction::PointerKind;",
        "pub use flui_interaction::PointerKindSet;",
        "pub use flui_interaction::PrimaryContact;",
        "pub use flui_interaction::cancel_all;",
        "pub use flui_interaction::focus::FocusNode;",
        "pub use flui_interaction::focus::FocusSubscription;",
        "pub use flui_interaction::gesture::CompositionError;",
        "pub use flui_interaction::gesture::CompositionKind;",
        "pub use flui_interaction::gesture::ContestCancel;",
        "pub use flui_interaction::gesture::GestureBranches;",
        "pub use flui_interaction::gesture::GestureComposition;",
        "pub use flui_interaction::gesture::GestureContest;",
        "pub use flui_interaction::gesture::MotionRole;",
        "pub use flui_interaction::gesture::ObservedMember;",
        "pub use flui_interaction::gesture::PanClaim;",
        "pub use flui_interaction::gesture::SequencedBranch;",
        "pub use flui_objects::PathClipConfiguration;",
        "pub use flui_objects::RenderPhysicalShape;",
        "pub use flui_objects::TranslationFraction;",
        "pub use flui_painting::Alignment;",
        "pub use flui_painting::Canvas;",
        "pub use flui_painting::DrawOp;",
        "pub use flui_painting::TextSizing;",
        "pub use flui_painting::paint::Clip;",
        "pub use flui_painting::paint::Paint;",
        "pub use flui_painting::paint::Path;",
        "pub use flui_painting::styling::Border;",
        "pub use flui_painting::styling::BorderRadius;",
        "pub use flui_painting::styling::BorderRadiusExt;",
        "pub use flui_painting::styling::BorderSide;",
        "pub use flui_painting::styling::BorderStyle;",
        "pub use flui_painting::styling::BoxDecoration;",
        "pub use flui_painting::styling::Color;",
        "pub use flui_painting::typography::FontWeight;",
        "pub use flui_painting::typography::TextDirection;",
        "pub use flui_painting::typography::TextStyle;",
        "pub use flui_platform_api::Brightness;",
        "pub use flui_platform_api::EventTime;",
        "pub use flui_platform_api::HapticFeedback;",
        "pub use flui_platform_api::InvalidLocale;",
        "pub use flui_platform_api::Locale;",
        "pub use flui_platform_api::SystemPreferences;",
        "pub use flui_platform_api::TargetPlatform;",
        "pub use flui_platform_api::input_capabilities::Availability;",
        "pub use flui_platform_api::input_capabilities::InputCapabilities;",
        "pub use flui_platform_api::input_capabilities::PointerKindSet;",
        "pub use flui_rendering::constraints::BoxConstraints;",
        "pub use flui_rendering::hit_testing::HitTestBehavior;",
        "pub use flui_rendering::input_regions::InputRegion;",
        "pub use flui_rendering::input_regions::InputRegionKind;",
        "pub use flui_rendering::input_regions::InputRegionSink;",
        "pub use flui_rendering::protocol::BoxProtocol;",
        "pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};",
        "pub use flui_scheduler::FrameSnapshot;",
        "pub use flui_view::*;",
        "pub use flui_widgets as widgets;",
    ];
    assert_eq!(declared_surface(), pinned);
}
