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
//! presets, system preferences and haptic feedback kinds, input regions, the
//! snap policies scrollables settle through, the anchored drag built on them,
//! the theme-free context menu the design systems style, and the collapsing
//! header the scroll channel moves.
//! Animation prelude imports also pin the shared package-author entry point.
//! The typed inspection model the flui-driver spec places in the SDK (ADR-0242 §4) follows:
//! the trait with its derive, the snapshot a package's tests read, the diff and the renderer.
//! `custom_recognizer` proves the extension surface with a recognizer written
//! against the SDK alone, and `behavioural_widget` a gesture widget whose
//! gesture and alternatives perform one intent, checked by
//! `assert_gesture_alternatives`.

#[path = "surface/behavioural_widget.rs"]
mod behavioural_widget;
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
        DrivenController as _, Interval as _, JumpAt as _, Keyframes as _, MotionPolicy as _,
        MotionSpec as _, MotionUpdate as _, PlaybackRate as _, RangeDefect as _, RepeatMode as _,
        RunCanceled as _, Stagger as _, StaggerOrigin as _, StatusSubscription as _, Steps as _,
        Tween as _, TwoWayConverter as _, ValueRange as _, Vsync as _, VsyncPublication as _,
        VsyncRegistration as _, VsyncRegistrationError as _, VsyncRetirement as _,
        VsyncUpdate as _, curves as _,
    };
    use flui_sdk::foundation::notifier::Listenable as _;
    use flui_sdk::foundation::observe::{
        ElementMounted as _, ElementMoved as _, ElementRebuilt as _, ElementUnmounted as _,
        TreeObserver as _,
    };
    // Typed inspection (ADR-0242): the trait, its derive and what a package's tests read.
    use flui_sdk::foundation::inspect::{
        Fields as _, Inspect as _, InspectId as _, InspectTree as _, Level as _, Link as _,
        SnapshotError as _, SnapshotNode as _, SnapshotQuery as _, ToValue as _, Unit as _,
        Value as _, diff as _, render as _, snapshot as _,
    };
    use flui_sdk::foundation::panic::{PanicRecovery as _, RecoveryScope as _};
    use flui_sdk::foundation::{
        ChangeNotifier as _, ElementId as _, Listenable as _, ListenerCallback as _,
        ListenerId as _, ListenerObserver as _, RebuildReason as _, TextScaleProfile as _,
        TextSizingIntent as _, ViewKey as _,
    };
    use flui_sdk::geometry::{EdgeInsets as _, RRect as _, Radius as _};
    use flui_sdk::hooks::FrameSnapshot as _;
    use flui_sdk::interaction::{
        ArenaMembership as _, BeginContactError as _, CancelOutcome as _, CompositionBranch as _,
        CompositionError as _, CompositionKind as _, ContactEnd as _, ContactId as _,
        ContactSnapshot as _, ContestCancel as _, DragDownDetails as _, EventTime as _,
        FocusNode as _, FocusSubscription as _, GestureArena as _, GestureArenaEntry as _,
        GestureArenaMember as _, GestureBranches as _, GestureComposition as _,
        GestureContest as _, GestureDisposition as _, GestureEndReason as _,
        GestureObservation as _, GestureObserver as _, GestureProfiles as _,
        GestureRecognizer as _, GestureSettings as _, GestureSettingsProvider as _,
        MotionRole as _, ObservedComposition as _, ObservedMember as _, ObserverHandle as _,
        PanClaim as _, PointerDispatch as _, PointerEvent as _, PointerEventExt as _,
        PointerId as _, PointerKind as _, PointerKindSet as _, PrimaryContact as _,
        RejectionReason as _, SequencedBranch as _, cancel_all as _,
    };
    // The slot locks and the swipe the placement table puts in the SDK.
    use flui_sdk::interaction::{
        AxisLock as _, PhysicalEdge as _, SwipeDetails as _, SwipeGestureRecognizer as _,
        SwipeStart as _, TransformLock as _,
    };
    // The identity the input pipeline gives each contact, wheel, key and
    // assistive sequence, which drag sessions and haptic requests carry.
    use flui_sdk::foundation::InputSequenceId as _;
    // A behavioural widget's gesture alternatives (ADR-0216).
    use flui_sdk::painting::Alignment as _;
    use flui_sdk::painting::{
        Border as _, BorderRadius as _, BorderRadiusExt as _, BorderSide as _, BorderStyle as _,
        BoxDecoration as _, BoxShadow as _, Color as _,
    };
    use flui_sdk::painting::{Canvas as _, DrawOp as _};
    use flui_sdk::painting::{Clip as _, ClipOp as _, Paint as _, Path as _, StrokeCap as _};
    use flui_sdk::painting::{
        FontWeight as _, TextDirection as _, TextSizing as _, TextStyle as _,
    };
    use flui_sdk::painting::{SemanticsRegion as _, SemanticsRegionKey as _};
    use flui_sdk::pipeline::{
        PathClipConfiguration as _, RenderPhysicalShape as _, ScrollDirection as _,
        TranslationFraction as _,
    };
    use flui_sdk::platform::{
        Brightness as _, DurationScale as _, GesturePreferences as _, HapticFeedback as _,
        HostNotice as _, InvalidLocale as _, InvalidPreference as _, Locale as _,
        MemoryPressureLevel as _, MotionPreference as _, NativeMouseGeometry as _,
        NativeTouchGeometry as _, SessionEndBlock as _, SessionEndRequest as _,
        SystemPreferences as _, TargetPlatform as _, TextWeightPreference as _,
        WheelPreferences as _, WheelStep as _, WindowCommandError as _, WindowMode as _,
        WindowState as _,
    };
    use flui_sdk::rendering::{
        BoxConstraints as _, BoxProtocol as _, HitTestBehavior as _, RenderError as _,
        RenderResult as _, RenderUpdateImpact as _,
    };
    use flui_sdk::view::dev_agent::{
        AgentAnswer as _, AgentFault as _, AgentWindow as _, DevAgentHook as _, HandleKind as _,
    };
    use flui_sdk::view::dev_reload::{DevReloadHook as _, ReloadEvent as _, ReloadWake as _};
    use flui_sdk::view::element::ElementKind as _;
    use flui_sdk::view::prelude::{BuildContext as _, InheritedData as _, StatelessView as _};
    use flui_sdk::view::{
        AnimatedView as _, AsyncDriver as _, BoxedView as _, BuildContext as _,
        BuildContextExt as _, Child as _, FieldMask as _, FrameWaker as _, GlobalKey as _,
        HostNoticeSubscription as _, InheritedData as _, InheritedView as _, IntoView as _,
        LifecycleHandle as _, PostFrameHandle as _, RebuildHandle as _, RebuildReason as _,
        RenderObjectContext as _, RenderView as _, StatefulView as _, View as _, ViewExt as _,
        ViewState as _, WindowClosed as _, WindowHandle as _, WindowSubscription as _,
        impl_animated_view as _, impl_inherited_view as _, impl_render_view as _,
        single_child_view_children as _,
    };
    use flui_sdk::widgets::__private::{
        AnchoredBox as _, FlightSeat as _, SeatClaim as _, SeatHold as _, TextFormFieldConfig as _,
        TextFormFieldCore as _,
    };
    use flui_sdk::widgets::animated::VsyncScope as _;
    use flui_sdk::widgets::icon::IconData as _;
    use flui_sdk::widgets::layout::PreferredSizeView as _;
    use flui_sdk::widgets::prelude::{BoxConstraints as _, BoxFit as _};
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, ActivationPressIntent as _, Align as _, AnchorSnap as _,
        AnchorSnapError as _, AnchoredDrag as _, AnchoredDragController as _,
        AnchoredDragState as _, AnchorsError as _, AnimatedBuilder as _, AppBuilder as _,
        Assertiveness as _, AutovalidateMode as _, BackIntent as _,
        BoxedLocalizationsDelegate as _, ButtonActivateIntent as _, CallbackAction as _,
        Center as _, ClipRect as _, ColoredBox as _, Column as _, ConstrainedBox as _,
        Container as _, ContentShape as _, CrossAxisAlignment as _, CustomMultiChildLayout as _,
        CustomPaint as _, CustomPainter as _, DecoratedBox as _, DefaultFocusRingStyle as _,
        DefaultTextStyle as _, Directionality as _, Disclosure as _, DisclosureState as _,
        DismissIntent as _, DragSlot as _, DraggableAnchors as _, DraggableAnchorsBuilder as _,
        EditableText as _, Expanded as _, ExpansionState as _, FadeTransition as _, Flexible as _,
        FloatingHeaderSnapConfiguration as _, Focus as _, FocusRing as _, FocusRingStyle as _,
        ForcePressSlot as _, FormFieldHandle as _, GestureArenaScope as _, GestureDetector as _,
        GestureRecognizerFactory as _, HeadingLevel as _, HeroMode as _, HitShape as _,
        HitSlop as _, HitTestBehavior as _, Icon as _, IconData as _, IconTheme as _,
        IconThemeData as _, InheritedTheme as _, IntrinsicWidth as _, InvalidViewportFraction as _,
        LayoutId as _, MainAxisAlignment as _, MainAxisSize as _, MediaQuery as _,
        MediaQueryData as _, MergeSemantics as _, MouseRegion as _, MultiChildLayoutContext as _,
        MultiChildLayoutDelegate as _, NavigatorHandle as _, NavigatorObserver as _,
        NumericRange as _, NumericRangeError as _, Offstage as _, Opacity as _, Padding as _,
        PageRoute as _, PageSnap as _, PanClaimRegion as _, PopupRoute as _, Positioned as _,
        PreferredSizeView as _, RawGestureDetector as _, RecognizerContext as _,
        RefreshController as _, RefreshIndicator as _, RefreshIntent as _, RouteAnimation as _,
        RouteResult as _, Row as _, SafeArea as _, SelectAllTextIntent as _, Semantics as _,
        SemanticsAnchor as _, SemanticsRole as _, SharedSnapPolicy as _, SizedBox as _,
        SlideTransition as _, Slider as _, SliderState as _, SliverPersistentHeader as _,
        SliverPersistentHeaderDelegate as _, SnapPolicy as _, Stack as _, StackFit as _,
        SubmitCallback as _, Table as _, TableCell as _, TableCellVerticalAlignment as _,
        TableColumnWidth as _, TableRow as _, Text as _, TextEditingController as _,
        TickerMode as _, Toggled as _, Transform as _, TransformSlot as _, ViewportFraction as _,
        WidgetState as _, WidgetStateConstraint as _, WidgetStateProperty as _, WidgetStates as _,
        WidgetStatesController as _, WidgetsApp as _,
    };
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, ActivityArc as _, ActivityIndicator as _, Align as _,
        AnimatedBuilder as _, AnimatedLerp as _, AppBuilder as _, Assertiveness as _,
        AutovalidateMode as _, BoxedLocalizationsDelegate as _, ButtonActivateIntent as _,
        ButtonChange as _, CallbackAction as _, Center as _, ClipRect as _, ColoredBox as _,
        Column as _, ConstrainedBox as _, Container as _, CrossAxisAlignment as _,
        CustomMultiChildLayout as _, CustomPaint as _, CustomPainter as _, DecoratedBox as _,
        DecoratedBoxTransition as _, DefaultTextStyle as _, Directionality as _, Disclosure as _,
        DisclosureState as _, EditableText as _, ExcludeSemantics as _, Expanded as _,
        ExpansionState as _, FadeTransition as _, FittedBox as _, Flexible as _,
        FloatingHeaderSnapConfiguration as _, Focus as _, FormFieldHandle as _,
        GestureDetector as _, HeadingLevel as _, HeroMode as _, HitTestBehavior as _, Icon as _,
        IconData as _, IconTheme as _, IconThemeData as _, IgnorePointer as _, InheritedTheme as _,
        IntrinsicWidth as _, LayoutBuilder as _, LayoutId as _, Listener as _,
        MainAxisAlignment as _, MainAxisSize as _, MediaQuery as _, MediaQueryData as _,
        MergeSemantics as _, MotionBuilder as _, MouseRegion as _, MultiChildLayoutContext as _,
        MultiChildLayoutDelegate as _, NavigatorHandle as _, NavigatorObserver as _,
        NumericRange as _, NumericRangeError as _, Offstage as _, Opacity as _, Padding as _,
        PageController as _, PageRoute as _, PageTransitionBuilder as _, PageTransitions as _,
        PageView as _, ParallaxPageTransition as _, PointerDispatch as _, PointerEvent as _,
        PopupRoute as _, Positioned as _, PreferredSizeView as _, RefreshIndicatorPhase as _,
        RefreshIndicatorStyle as _, RouteAnimation as _, RouteResult as _, Row as _, SafeArea as _,
        ScaleTransition as _, SelectAllTextIntent as _, Semantics as _, SemanticsAnchor as _,
        SemanticsRole as _, SizedBox as _, SlideTransition as _, Slider as _, SliderState as _,
        SliverFadeTransition as _, SliverPersistentHeader as _,
        SliverPersistentHeaderDelegate as _, Stack as _, StackFit as _, SubmitCallback as _,
        Table as _, TableCell as _, TableCellVerticalAlignment as _, TableColumnWidth as _,
        TableRow as _, Text as _, TextEditingController as _, TickerMode as _, Toggled as _,
        Transform as _, WidgetState as _, WidgetStateConstraint as _, WidgetStateProperty as _,
        WidgetStates as _, WidgetStatesController as _, WidgetsApp as _,
    };
    use flui_sdk::widgets::{
        Alternative as _, AlternativeKind as _, CommandAppearance as _, CommandHandle as _,
        CompanionGap as _, CompanionLink as _, CompanionRegistration as _, CompanionSpec as _,
        ContactAdmission as _, ContextMenuArea as _, ControlPlacement as _, ControlSpec as _,
        GestureAlternatives as _, GestureAlternativesError as _, GestureCommand as _,
        InteractiveViewer as _, MenuCause as _, MenuItem as _, MenuPresenter as _,
        MenuRequest as _, MenuSpec as _, PanIntent as _, ShowContextMenuIntent as _,
        SingleActivator as _, WidgetsLocalizations as _, ZoomIntent as _,
    };
    use flui_sdk::widgets::{CellPosition as _, SetSize as _, SortDirection as _, TableShape as _};
    use flui_sdk::widgets::{
        CollapsingHeader as _, CollapsingHeaderState as _, HeaderExtents as _,
        HeaderExtentsError as _, HeaderOrder as _,
    };
    use flui_sdk::widgets::{
        CopySelectionTextIntent as _, DragAnchor as _, DragCommands as _, Draggable as _,
        DropCommands as _, DropHover as _, DropTarget as _, PasteTextIntent as _,
    };
    use flui_sdk::widgets::{
        DismissDirection as _, DismissSide as _, DismissSideCallback as _, DismissThreshold as _,
        DismissThresholdError as _, DismissUpdateCallback as _, DismissUpdateDetails as _,
        Dismissible as _, DismissibleState as _, SwipeAction as _, SwipeActions as _,
        SwipeActionsError as _, SwipeActionsState as _, SwipeAlternative as _, SwipeEdge as _,
    };
    use flui_sdk::widgets::{MultiTapCount as _, MultiTapCountError as _};
    use flui_sdk::{
        geometry::{
            EdgeInsets as _, Lerp as _, Offset as _, Point as _, RRect as _, Rect as _, Size as _,
        },
        painting::{Alignment as _, Color as _},
    };

    /// A method a package calls on a framework type that has no import of
    /// its own: `OpenContainer` keeps the page below still with it.
    const _: fn(flui_sdk::widgets::PageRoute<()>, bool) -> flui_sdk::widgets::PageRoute<()> =
        flui_sdk::widgets::PageRoute::animates_route_below;

    /// `OpenContainer` asks it whether to push a page or, under a `Router`
    /// whose navigator refuses pages pushed through its handle, a popup.
    const _: fn(&flui_sdk::widgets::NavigatorHandle) -> bool =
        flui_sdk::widgets::NavigatorHandle::admits_pages;
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
    let _: fn(flui::interaction::drag::DragItem) -> flui_sdk::interaction::drag::DragItem = |x| x;
    let _: fn(flui::interaction::drag::DragAndDrop) -> flui_sdk::interaction::drag::DragAndDrop =
        |x| x;
    let _: fn(
        Box<dyn flui::interaction::drag::DropReceiver>,
    ) -> Box<dyn flui_sdk::interaction::drag::DropReceiver> = |x| x;
    let _: fn(flui::widgets::Draggable) -> flui_sdk::widgets::Draggable = |x| x;
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
    let _: fn(flui::interaction::AxisLock) -> flui_sdk::interaction::AxisLock = |x| x;
    let _: fn(flui::interaction::TransformLock) -> flui_sdk::interaction::TransformLock = |x| x;
    let _: fn(flui::interaction::SwipeStart) -> flui_sdk::interaction::SwipeStart = |x| x;
    let _: fn(
        flui::interaction::SwipeGestureRecognizer,
    ) -> flui_sdk::interaction::SwipeGestureRecognizer = |x| x;
    let _: fn(
        Box<dyn flui::interaction::GestureRecognizer>,
    ) -> Box<dyn flui_sdk::interaction::GestureRecognizer> = |x| x;
    let _: fn(flui::widgets::RecognizerContext) -> flui_sdk::widgets::RecognizerContext = |x| x;
    let _: fn(flui::platform::SystemPreferences) -> flui_sdk::platform::SystemPreferences = |x| x;
    let _: fn(flui::platform::MotionPreference) -> flui_sdk::platform::MotionPreference = |x| x;
    let _: fn(flui::platform::DurationScale) -> flui_sdk::platform::DurationScale = |x| x;
    let _: fn(flui::platform::TextWeightPreference) -> flui_sdk::platform::TextWeightPreference =
        |x| x;
    let _: fn(flui::platform::GesturePreferences) -> flui_sdk::platform::GesturePreferences = |x| x;
    let _: fn(flui::platform::NativeMouseGeometry) -> flui_sdk::platform::NativeMouseGeometry =
        |x| x;
    let _: fn(flui::platform::NativeTouchGeometry) -> flui_sdk::platform::NativeTouchGeometry =
        |x| x;
    let _: fn(flui::platform::WheelPreferences) -> flui_sdk::platform::WheelPreferences = |x| x;
    let _: fn(flui::platform::WheelStep) -> flui_sdk::platform::WheelStep = |x| x;
    let _: fn(flui::platform::InvalidPreference) -> flui_sdk::platform::InvalidPreference = |x| x;
    let _: fn(flui::platform::TargetPlatform) -> flui_sdk::platform::TargetPlatform = |x| x;
    let _: fn(flui::platform::HapticFeedback) -> flui_sdk::platform::HapticFeedback = |x| x;
    let _: fn(flui::painting::Canvas) -> flui_sdk::painting::Canvas = |x| x;
    let _: fn(flui::painting::SemanticsRegion) -> flui_sdk::painting::SemanticsRegion = |x| x;
    let _: fn(flui::painting::SemanticsRegionKey) -> flui_sdk::painting::SemanticsRegionKey = |x| x;
    let _: fn(flui::painting::DrawOp) -> flui_sdk::painting::DrawOp = |x| x;
    let _: fn(flui::painting::TextSizing) -> flui_sdk::painting::TextSizing = |x| x;
    let _: fn(flui::platform::Brightness) -> flui_sdk::platform::Brightness = |x| x;
    let _: fn(flui::platform::InvalidLocale) -> flui_sdk::platform::InvalidLocale = |x| x;
    let _: fn(flui::platform::TargetPlatform) -> flui_sdk::platform::TargetPlatform = |x| x;
    let _: fn(flui::rendering::RenderUpdateImpact) -> flui_sdk::rendering::RenderUpdateImpact =
        |x| x;
    let _: fn(flui::rendering::RenderError) -> flui_sdk::rendering::RenderError = |x| x;
    let _: fn(flui::rendering::RenderResult<()>) -> flui_sdk::rendering::RenderResult<()> = |x| x;
    let _: fn(flui::rendering::BoxConstraints) -> flui_sdk::rendering::BoxConstraints = |x| x;
    let _: fn(flui::rendering::HitTestBehavior) -> flui_sdk::rendering::HitTestBehavior = |x| x;
    let _: fn(flui::rendering::BoxProtocol) -> flui_sdk::rendering::BoxProtocol = |x| x;
}

/// The unstable input surface a package opts into with `unstable-input`:
/// each item resolves and is the owning crate's own. Compared with the owning
/// crates rather than the facade, whose dev edge here stays without the
/// feature so the workspace's doctests keep checking that the facade hides
/// these modules by default. CI's per-feature test pass builds this.
#[cfg(feature = "unstable-input")]
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod measured_unstable {
    use flui_sdk::interaction::{HapticObserver as _, HapticsHandle as _};
    use flui_sdk::platform::haptics::{
        ControlPoint as _, HapticCapabilities as _, HapticEvent as _, HapticOutcome as _,
        HapticPattern as _, HapticPatternBuilder as _, HapticPatternError as _, HapticRequest as _,
        HapticTicket as _, HapticTiming as _, Intensity as _, ParameterCurve as _,
        PatternPlayback as _, Sharpness as _, UnitRangeError as _,
    };
    use flui_sdk::platform::input_capabilities::{
        Availability as _, InputCapabilities as _, PointerKindSet as _,
    };
}

#[cfg(feature = "unstable-input")]
#[test]
fn the_unstable_input_re_exports_are_the_owning_crates_types() {
    let _: fn(
        flui_platform::input_capabilities::InputCapabilities,
    ) -> flui_sdk::platform::input_capabilities::InputCapabilities = |x| x;
    let _: fn(flui_platform::Availability) -> flui_sdk::platform::input_capabilities::Availability =
        |x| x;
    let _: fn(
        flui_platform::input_capabilities::PointerKindSet,
    ) -> flui_sdk::platform::input_capabilities::PointerKindSet = |x| x;
    let _: fn(flui_platform::haptics::HapticPattern) -> flui_sdk::platform::haptics::HapticPattern =
        |x| x;
    let _: fn(flui_platform::haptics::HapticRequest) -> flui_sdk::platform::haptics::HapticRequest =
        |x| x;
    let _: fn(flui_platform::haptics::HapticOutcome) -> flui_sdk::platform::haptics::HapticOutcome =
        |x| x;
    let _: fn(flui_interaction::HapticsHandle) -> flui_sdk::interaction::HapticsHandle = |x| x;
    let _: fn(flui_interaction::HapticObserver) -> flui_sdk::interaction::HapticObserver = |x| x;
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
        "pub mod drag {",
        "pub mod haptics {",
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
        "pub use flui_interaction::AxisLock;",
        "pub use flui_interaction::BeginContactError;",
        "pub use flui_interaction::CancelOutcome;",
        "pub use flui_interaction::ContactEnd;",
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
        "pub use flui_interaction::HapticObserver;",
        "pub use flui_interaction::HapticsHandle;",
        "pub use flui_interaction::ObserverHandle;",
        "pub use flui_interaction::PhysicalEdge;",
        "pub use flui_interaction::PointerDispatch;",
        "pub use flui_interaction::PointerEvent;",
        "pub use flui_interaction::PointerEventExt;",
        "pub use flui_interaction::PointerId;",
        "pub use flui_interaction::PointerKind;",
        "pub use flui_interaction::PointerKindSet;",
        "pub use flui_interaction::PrimaryContact;",
        "pub use flui_interaction::SwipeDetails;",
        "pub use flui_interaction::SwipeGestureRecognizer;",
        "pub use flui_interaction::SwipeStart;",
        "pub use flui_interaction::TransformLock;",
        "pub use flui_interaction::cancel_all;",
        "pub use flui_interaction::drag::AllowedActions;",
        "pub use flui_interaction::drag::Autoscroll;",
        "pub use flui_interaction::drag::DragAndDrop;",
        "pub use flui_interaction::drag::DragError;",
        "pub use flui_interaction::drag::DragIdentity;",
        "pub use flui_interaction::drag::DragItem;",
        "pub use flui_interaction::drag::DragObserver;",
        "pub use flui_interaction::drag::DragOutcome;",
        "pub use flui_interaction::drag::DragPoint;",
        "pub use flui_interaction::drag::DragReach;",
        "pub use flui_interaction::drag::DragSession;",
        "pub use flui_interaction::drag::DragStart;",
        "pub use flui_interaction::drag::DragTrigger;",
        "pub use flui_interaction::drag::DragView;",
        "pub use flui_interaction::drag::DropReceiver;",
        "pub use flui_interaction::drag::DropReceiverSlot;",
        "pub use flui_interaction::drag::MoveOutcome;",
        "pub use flui_interaction::drag::SpringLoading;",
        "pub use flui_interaction::focus::FocusNode;",
        "pub use flui_interaction::focus::FocusSubscription;",
        "pub use flui_interaction::gesture::CompositionBranch;",
        "pub use flui_interaction::gesture::CompositionError;",
        "pub use flui_interaction::gesture::CompositionKind;",
        "pub use flui_interaction::gesture::ContestCancel;",
        "pub use flui_interaction::gesture::GestureBranches;",
        "pub use flui_interaction::gesture::GestureComposition;",
        "pub use flui_interaction::gesture::GestureContest;",
        "pub use flui_interaction::gesture::MotionRole;",
        "pub use flui_interaction::gesture::ObservedComposition;",
        "pub use flui_interaction::gesture::ObservedMember;",
        "pub use flui_interaction::gesture::PanClaim;",
        "pub use flui_interaction::gesture::RejectionReason;",
        "pub use flui_interaction::gesture::SequencedBranch;",
        "pub use flui_objects::PathClipConfiguration;",
        "pub use flui_objects::RenderPhysicalShape;",
        "pub use flui_objects::TranslationFraction;",
        "pub use flui_painting::Alignment;",
        "pub use flui_painting::Canvas;",
        "pub use flui_painting::DrawOp;",
        "pub use flui_painting::TextSizing;",
        "pub use flui_painting::paint::Clip;",
        "pub use flui_painting::paint::ClipOp;",
        "pub use flui_painting::paint::Paint;",
        "pub use flui_painting::paint::Path;",
        "pub use flui_painting::paint::StrokeCap;",
        "pub use flui_painting::styling::Border;",
        "pub use flui_painting::styling::BorderRadius;",
        "pub use flui_painting::styling::BorderRadiusExt;",
        "pub use flui_painting::styling::BorderSide;",
        "pub use flui_painting::styling::BorderStyle;",
        "pub use flui_painting::styling::BoxDecoration;",
        "pub use flui_painting::styling::BoxShadow;",
        "pub use flui_painting::styling::Color;",
        "pub use flui_painting::typography::FontWeight;",
        "pub use flui_painting::typography::TextDirection;",
        "pub use flui_painting::typography::TextStyle;",
        "pub use flui_platform::Availability;",
        "pub use flui_platform::Brightness;",
        "pub use flui_platform::DurationScale;",
        "pub use flui_platform::EventTime;",
        "pub use flui_platform::GesturePreferences;",
        "pub use flui_platform::HapticFeedback;",
        "pub use flui_platform::HostNotice;",
        "pub use flui_platform::InvalidLocale;",
        "pub use flui_platform::InvalidPreference;",
        "pub use flui_platform::Locale;",
        "pub use flui_platform::MemoryPressureLevel;",
        "pub use flui_platform::MotionPreference;",
        "pub use flui_platform::NativeMouseGeometry;",
        "pub use flui_platform::NativeTouchGeometry;",
        "pub use flui_platform::SessionEndBlock;",
        "pub use flui_platform::SessionEndRequest;",
        "pub use flui_platform::SystemPreferences;",
        "pub use flui_platform::TargetPlatform;",
        "pub use flui_platform::TextWeightPreference;",
        "pub use flui_platform::Unsupported;",
        "pub use flui_platform::UnsupportedReason;",
        "pub use flui_platform::WheelPreferences;",
        "pub use flui_platform::WheelStep;",
        "pub use flui_platform::WindowCommandError;",
        "pub use flui_platform::WindowMode;",
        "pub use flui_platform::WindowState;",
        "pub use flui_platform::data_transfer::TransferActions;",
        "pub use flui_platform::haptics::ControlPoint;",
        "pub use flui_platform::haptics::HapticCapabilities;",
        "pub use flui_platform::haptics::HapticEvent;",
        "pub use flui_platform::haptics::HapticOutcome;",
        "pub use flui_platform::haptics::HapticPattern;",
        "pub use flui_platform::haptics::HapticPatternBuilder;",
        "pub use flui_platform::haptics::HapticPatternError;",
        "pub use flui_platform::haptics::HapticRequest;",
        "pub use flui_platform::haptics::HapticTicket;",
        "pub use flui_platform::haptics::HapticTiming;",
        "pub use flui_platform::haptics::Intensity;",
        "pub use flui_platform::haptics::ParameterCurve;",
        "pub use flui_platform::haptics::PatternPlayback;",
        "pub use flui_platform::haptics::Sharpness;",
        "pub use flui_platform::haptics::UnitRangeError;",
        "pub use flui_platform::input_capabilities::InputCapabilities;",
        "pub use flui_platform::input_capabilities::PointerKindSet;",
        "pub use flui_platform::text_store::TextInputPurpose;",
        "pub use flui_rendering::constraints::BoxConstraints;",
        "pub use flui_rendering::hit_testing::HitTestBehavior;",
        "pub use flui_rendering::protocol::BoxProtocol;",
        "pub use flui_rendering::semantics::SemanticsRegion;",
        "pub use flui_rendering::semantics::SemanticsRegionKey;",
        "pub use flui_rendering::view::ScrollDirection;",
        "pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};",
        "pub use flui_scheduler::FrameSnapshot;",
        "pub use flui_view::*;",
        "pub use flui_widgets as widgets;",
    ];
    assert_eq!(declared_surface(), pinned);
}
