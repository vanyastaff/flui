//! The SDK's surface is the measured list, and each re-export is the item it
//! names, not a copy.
//!
//! The list starts with the package imports measured in ARCHITECTURE.md and
//! pins subsequent consumer additions and removals, plus `painting::DrawOp`,
//! which the design systems' paint tests read, `painting::Shadow` and
//! `painting::BlurSigma`, which a text style's shadows and the op a
//! paragraph's shadow records name (ADR-0309), the four decoration types
//! `TextStyle::decoration` names (ADR-0310), `view::dev_reload`, the
//! hook `flui-hot-reload`'s host drivers implement (ADR-0311), and
//! `view::dev_agent`, the hook a package's development tool implements
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
#[path = "surface/package_capability.rs"]
mod package_capability;

/// Every measured item through its SDK path: removing or moving one fails to
/// build this test.
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod measured {
    use flui_sdk::animation::prelude::{
        Animatable as _, AnimatedValue as _, Animation as _, AnimationController as _,
        AnimationStatus as _, Curve as _, DrivenController as _, Keyframes as _, Listenable as _,
        MotionSpec as _, SharedCurve as _, Tween as _, TwoWayConverter as _,
    };
    use flui_sdk::animation::{
        Animatable as _, AnimatedValue as _, AnimatedValueView as _, Animation as _,
        AnimationBehavior as _, AnimationController as _, AnimationError as _, AnimationInput as _,
        AnimationRunFuture as _, AnimationStatus as _, AnimationVector as _,
        ConstantAnimation as _, Cubic as _, Curve as _, CurvedAnimation as _,
        DrivenController as _, Interval as _, JumpAt as _, Keyframes as _, MotionPolicy as _,
        MotionSpec as _, MotionUpdate as _, PlaybackRate as _, RangeDefect as _, RepeatMode as _,
        RunCanceled as _, SharedCurve as _, Stagger as _, StaggerOrigin as _,
        StatusSubscription as _, Steps as _, Tween as _, TwoWayConverter as _, ValueRange as _,
        Vsync as _, VsyncPublication as _, VsyncRegistration as _, VsyncRegistrationError as _,
        VsyncRetirement as _, VsyncUpdate as _, curves as _,
    };
    // The spring a loading indicator's morph follows.
    use flui_sdk::animation::{
        Simulation as _, SimulationError as _, SpringDescription as _, SpringSimulation as _,
        Tolerance as _,
    };
    use flui_sdk::foundation::diagnostics::{
        FrameSpan as _, PERIOD_US as _, PRESENTATION as _, RUNTIME as _,
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
    use flui_sdk::interaction::{
        ArenaMembership as _, BeginContactError as _, ButtonChange as _, CancelOutcome as _,
        CompositionBranch as _, CompositionError as _, CompositionKind as _, ContactEnd as _,
        ContactId as _, ContactSnapshot as _, ContestCancel as _, CursorIcon as _, DeviceId as _,
        DragDownDetails as _, DragEndDetails as _, DragStartDetails as _, DragUpdateDetails as _,
        EventPropagation as _, EventTime as _, FocusNode as _,
        GestureArena as _, GestureArenaEntry as _, GestureArenaMember as _, GestureBranches as _,
        GestureComposition as _, GestureContest as _, GestureDisposition as _,
        GestureEndReason as _, GestureObservation as _, GestureObserver as _, GestureProfiles as _,
        GestureRecognizer as _, GestureSettings as _, GestureSettingsProvider as _,
        HitTestBehavior as _, MotionRole as _, ObservedComposition as _, ObservedMember as _,
        ObserverHandle as _, PanClaim as _, PanZoomEvent as _, PointerDispatch as _,
        PointerEvent as _, PointerEventExt as _, PointerId as _, PointerKind as _,
        PointerKindSet as _, PrimaryContact as _, RejectionReason as _, SequencedBranch as _,
        TapDetails as _, cancel_all as _,
    };
    // Keyboard and pointer values a package's interaction tests construct.
    use flui_sdk::interaction::{
        Code as _, Key as _, KeyEvent as _, KeyEventResult as _, KeyState as _, NamedKey as _,
        PointerButton as _, PointerButtons as _, PointerPress as _, PointerRelease as _,
    };
    // What a text widget's store is built from (ADR-0151 §6).
    use flui_sdk::interaction::text_input::{
        CompositionLedger as _, EditGeneration as _, LockArbiter as _, committed_text as _,
    };
    // The slot locks and the swipe the placement table puts in the SDK.
    use flui_sdk::interaction::{
        AxisLock as _, PhysicalEdge as _, SwipeDetails as _, SwipeGestureRecognizer as _,
        SwipeStart as _, TransformLock as _,
    };
    // The identity the input pipeline gives each contact, wheel, key and
    // assistive sequence, which drag sessions and haptic requests carry.
    use flui_sdk::foundation::InputSequenceId as _;
    use flui_sdk::painting::paint::{
        Clip as _, ClipOp as _, ImageRepeat as _, Morph as _, Path as _, PathCommand as _,
        RoundedPolygon as _, material_shapes as _,
    };
    use flui_sdk::painting::{
        Alignment as _, BlurSigma as _, Canvas as _, CustomPainter as _, DrawOp as _,
        ImagePlacement as _, Paint as _, SemanticsRegion as _, SemanticsRegionKey as _,
        StrokeCap as _, TextSizing as _,
    };
    // A text style's shadows and the op a paragraph's shadow records (ADR-0309).
    use flui_sdk::painting::styling::{
        Border as _, BorderRadius as _, BorderRadiusExt as _, BorderSide as _, BorderStyle as _,
        BoxDecoration as _, BoxShadow as _, Color as _, Shadow as _, TableBorder as _,
    };
    // A text style's decoration (ADR-0310) and the Material 3 Expressive shapes.
    use flui_sdk::painting::typography::{
        DecorationThickness as _, FontWeight as _, TextDecoration as _, TextDecorationLines as _,
        TextDecorationStyle as _, TextDirection as _, TextSpan as _, TextStyle as _,
    };
    use flui_sdk::pipeline::{
        CarouselLayout as _, CrossAxisAlignment as _, FloatingHeaderSnapConfiguration as _,
        FocusRingStyle as _, HitShape as _, MainAxisAlignment as _, MainAxisSize as _,
        PathClipConfiguration as _, RenderPhysicalShape as _, ScrollDirection as _, StackFit as _,
        TableColumnWidth as _, TranslationFraction as _,
    };
    // The capability seam a package declares, provides and acquires with
    // (ADR-0152), the clipboard, its first capability, and the primary
    // selection.
    use flui_sdk::interaction::{
        ClipboardBackend as _, ClipboardCapability as _, ClipboardHandle as _, PrimaryClaim as _,
        PrimaryClaimId as _, PrimaryClaims as _, PrimarySelectionBackend as _,
        PrimarySelectionCapability as _, PrimarySelectionHandle as _, SelectionText as _,
    };
    use flui_sdk::interaction::{MultiTapCount as _, MultiTapCountError as _};
    use flui_sdk::platform::capability::{
        Abandoned as _, Capability as _, CapabilityProvider as _, CapabilityRegistrar as _,
        Completion as _, CompletionSource as _, OwnerPoster as _, OwnerRetired as _, Plugin as _,
        ProviderContext as _, Resolver as _, WindowRef as _,
    };
    use flui_sdk::platform::text_store::TextInputPurpose as _;
    use flui_sdk::platform::{
        AdmissionError as _, Availability as _, Brightness as _, ClipboardError as _,
        DurationScale as _, GesturePreferences as _, HapticFeedback as _, HostNotice as _,
        InvalidLocale as _, InvalidPreference as _, Locale as _, MemoryPressureLevel as _,
        MotionPreference as _, NativeMouseGeometry as _, NativeTouchGeometry as _,
        SessionEndBlock as _, SessionEndRequest as _, SubmitError as _, SystemPreferences as _,
        TargetPlatform as _, TextWeightPreference as _, WheelPreferences as _, WheelStep as _,
        WindowMode as _, WindowState as _,
    };
    use flui_sdk::rendering::{
        ActionCall as _, AnnounceError as _, Announcer as _, AutoComplete as _,
        BoxConstraints as _, BoxProtocol as _, CellPosition as _, Current as _, FlexFit as _,
        HasPopup as _, HeadingLevel as _, Invalid as _, LiveSetting as _,
        MultiChildLayoutContext as _, MultiChildLayoutDelegate as _, NumericRange as _,
        NumericRangeError as _, RenderError as _, RenderResult as _, RenderUpdateImpact as _,
        Reveal as _, ScrollPosition as _, ScrollUnit as _, SemanticsAnchor as _,
        SemanticsConfiguration as _, SemanticsRole as _, SetSize as _, SortDirection as _,
        TableCellVerticalAlignment as _, TableShape as _, TextDirection as _, TextSelection as _,
        Toggled as _, Urgency as _,
    };
    use flui_sdk::view::LifecycleContextExt as _;
    use flui_sdk::view::dev_agent::{
        AgentAnswer as _, AgentEvents as _, AgentWindow as _, DevAgentHook as _,
    };
    use flui_sdk::view::dev_reload::{DevReloadHook as _, ReloadEvent as _, ReloadWake as _};
    use flui_sdk::view::element::ElementKind as _;
    use flui_sdk::view::environment::{GestureArenaScope as _, VsyncScope as _};
    use flui_sdk::view::environment::{MediaQuery as _, MediaQueryData as _};
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
    use flui_sdk::widgets::icon::IconData as _;
    use flui_sdk::widgets::layout::PreferredSizeView as _;
    use flui_sdk::widgets::prelude::{BoxConstraints as _, BoxFit as _};
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, ActivationPressIntent as _, Align as _, AnchorSnap as _,
        AnchorSnapError as _, AnchoredDrag as _, AnchoredDragController as _,
        AnchoredDragState as _, AnchorsError as _, AnimatedBuilder as _, AppBuilder as _,
        AutovalidateMode as _, BackIntent as _, BoxedLocalizationsDelegate as _,
        ButtonActivateIntent as _, CallbackAction as _, Center as _, ClipRect as _,
        ColoredBox as _, Column as _, ConstrainedBox as _, Container as _, ContentShape as _,
        CustomMultiChildLayout as _, CustomPaint as _, DecoratedBox as _,
        DefaultFocusRingStyle as _, DefaultTextStyle as _, Directionality as _, Disclosure as _,
        DisclosureState as _, DismissIntent as _, DragSlot as _, DraggableAnchors as _,
        DraggableAnchorsBuilder as _, EditableText as _, Expanded as _, ExpansionState as _,
        FadeTransition as _, Flexible as _, Focus as _, FocusRing as _, ForcePressSlot as _,
        FormFieldHandle as _, GestureDetector as _, GestureRecognizerFactory as _, HeroMode as _,
        HitSlop as _, Icon as _, IconData as _, IconTheme as _, IconThemeData as _,
        InheritedTheme as _, IntrinsicWidth as _, InvalidViewportFraction as _, LayoutId as _,
        MergeSemantics as _, MouseRegion as _, NavigatorHandle as _, NavigatorObserver as _,
        Offstage as _, Opacity as _, Padding as _, PageRoute as _, PageSnap as _,
        PanClaimRegion as _, PopupRoute as _, Positioned as _, PreferredSizeView as _,
        RawGestureDetector as _, RecognizerContext as _, RefreshController as _,
        RefreshIndicator as _, RefreshIntent as _, RouteAnimation as _, RouteResult as _,
        RouteResultError as _, Row as _, SafeArea as _, SelectAllTextIntent as _, Semantics as _,
        SharedSnapPolicy as _, SizedBox as _, SlideTransition as _, Slider as _, SliderState as _,
        SliverPersistentHeader as _, SliverPersistentHeaderDelegate as _, SnapPolicy as _,
        Stack as _, SubmitCallback as _, Table as _, TableCell as _, TableRow as _, Text as _,
        TextEditingController as _, TickerMode as _, Transform as _, TransformSlot as _,
        ViewportFraction as _, WidgetState as _, WidgetStateConstraint as _,
        WidgetStateProperty as _, WidgetStates as _, WidgetStatesController as _, WidgetsApp as _,
    };
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, ActivityArc as _, ActivityIndicator as _, Align as _,
        AnimatedBuilder as _, AnimatedLerp as _, AppBuilder as _, AutovalidateMode as _,
        BoxedLocalizationsDelegate as _, ButtonActivateIntent as _, CallbackAction as _,
        Carousel as _, CarouselAutoplay as _, CarouselController as _, CarouselItemScope as _,
        Center as _, ClipRect as _, ColoredBox as _, Column as _, ConstrainedBox as _,
        Container as _, CustomMultiChildLayout as _, CustomPaint as _, DecoratedBox as _,
        DecoratedBoxTransition as _, DefaultTextStyle as _, Directionality as _, Disclosure as _,
        DisclosureState as _, EditableText as _, ExcludeSemantics as _, Expanded as _,
        ExpansionState as _, FadeTransition as _, FittedBox as _, Flexible as _, Focus as _,
        FormFieldHandle as _, GestureDetector as _, HeroMode as _, Icon as _, IconData as _,
        IconTheme as _, IconThemeData as _, IgnorePointer as _, InheritedTheme as _,
        IntrinsicWidth as _, KeylineSnap as _, LayoutBuilder as _, LayoutId as _, Listener as _,
        MergeSemantics as _, MotionBuilder as _, MouseRegion as _, NavigatorHandle as _,
        NavigatorObserver as _, Offstage as _, Opacity as _, Padding as _, PageController as _,
        PageRoute as _, PageTransitionBuilder as _, PageTransitions as _, PageView as _,
        ParallaxPageTransition as _, PopupRoute as _, Positioned as _, PreferredSizeView as _,
        RefreshIndicatorPhase as _, RefreshIndicatorStyle as _, RouteAnimation as _,
        RouteResult as _, RouteResultError as _, Row as _, SafeArea as _, ScaleTransition as _,
        SelectAllTextIntent as _, Semantics as _, SizedBox as _, SlideTransition as _, Slider as _,
        SliderState as _, SliverFadeTransition as _, SliverPersistentHeader as _,
        SliverPersistentHeaderDelegate as _, Stack as _, SubmitCallback as _, Table as _,
        TableCell as _, TableRow as _, Text as _, TextEditingController as _, TickerMode as _,
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
    use flui_sdk::widgets::{
        CollapsingHeader as _, CollapsingHeaderState as _, HeaderExtents as _,
        HeaderExtentsError as _, HeaderOrder as _,
    };
    use flui_sdk::widgets::{
        CopySelectionTextIntent as _, DragAnchor as _, DragCommands as _, Draggable as _,
        DropCommands as _, DropHover as _, DropTarget as _, PasteTextIntent as _,
    };
    use flui_sdk::widgets::{
        DiscreteEffect as _, IndefiniteEffect as _, PathEffect as _, RotateEffect as _, Speed as _,
        SymbolEffect as _,
    };
    use flui_sdk::widgets::{
        DismissDirection as _, DismissSide as _, DismissSideCallback as _, DismissThreshold as _,
        DismissThresholdError as _, DismissUpdateCallback as _, DismissUpdateDetails as _,
        Dismissible as _, DismissibleState as _, SwipeAction as _, SwipeActions as _,
        SwipeActionsError as _, SwipeActionsState as _, SwipeAlternative as _, SwipeEdge as _,
    };
    use flui_sdk::widgets::{
        PageIndicator as _, PageIndicatorState as _, PageIndicatorStyle as _,
        PageIndicatorStyleError as _,
    };
    use flui_sdk::widgets::{
        Reorder as _, ReorderCause as _, ReorderHandles as _, ReorderIntent as _,
        ReorderPreview as _, ReorderableGrid as _, ReorderableGridState as _, ReorderableItem as _,
        ReorderableList as _, ReorderableListState as _,
    };
    use flui_sdk::{
        geometry::{
            EdgeInsets as _, Lerp as _, Offset as _, Point as _, RRect as _, Rect as _, Size as _,
        },
        painting::{Alignment as _, styling::Color as _},
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

macro_rules! same_path {
    ($($p:ident)::+ $(<$T:ty>)?) => {
        let _: fn(flui::$($p)::+ $(<$T>)?) -> flui_sdk::$($p)::+ $(<$T>)? = |x| x;
    };
}

macro_rules! same_boxed_path {
    ($($p:ident)::+) => {
        let _: fn(Box<dyn flui::$($p)::+>) -> Box<dyn flui_sdk::$($p)::+> = |x| x;
    };
}

/// Each curated item, and one type per whole-module re-export, is the facade's
/// own type at the same path: this fails to build if an SDK path diverges,
/// becomes a wrapper, a newtype or a different item of the same name.
#[test]
fn the_re_exports_are_the_facades_types() {
    same_path!(animation::AnimationController);
    same_path!(animation::prelude::DrivenController);
    same_path!(animation::prelude::AnimatedValue<f64>);
    same_path!(animation::prelude::AnimatedValueView<f64>);
    same_path!(animation::prelude::AnimationRunFuture);
    same_path!(animation::prelude::MotionSpec);
    same_path!(animation::prelude::Keyframes<f64>);
    same_path!(foundation::ElementId);
    same_path!(geometry::Rect);
    same_path!(view::RebuildHandle);
    same_path!(view::WidgetsBinding);
    same_path!(view::dev_reload::ReloadWake);
    same_path!(view::persist::SaveStatus);
    same_path!(widgets::Text);

    same_path!(interaction::GestureEndReason);
    same_path!(interaction::DragDownDetails);
    same_path!(interaction::FocusNode);
    same_path!(interaction::GestureProfiles);
    same_path!(interaction::GestureSettings);
    same_path!(interaction::GestureArena);
    same_path!(interaction::PrimaryContact);
    same_path!(interaction::GestureComposition);
    same_path!(interaction::SequencedBranch);
    same_path!(interaction::drag::DragItem);
    same_path!(interaction::drag::DragAndDrop);
    same_boxed_path!(interaction::drag::DropReceiver);
    same_path!(widgets::Draggable);
    same_path!(interaction::MotionRole);
    same_path!(interaction::PanClaim);
    same_path!(interaction::PointerKindSet);
    same_path!(interaction::GestureSettingsProvider);
    same_boxed_path!(interaction::GestureObserver);
    same_path!(interaction::EventTime);
    same_path!(interaction::AxisLock);
    same_path!(interaction::TransformLock);
    same_path!(interaction::SwipeStart);
    same_path!(interaction::SwipeGestureRecognizer);
    same_boxed_path!(interaction::GestureRecognizer);
    same_path!(interaction::text_input::LockArbiter);
    same_path!(widgets::RecognizerContext);
    same_path!(interaction::Code);
    same_path!(interaction::Key);
    same_path!(interaction::KeyEvent);
    same_path!(interaction::KeyEventResult);
    same_path!(interaction::KeyState);
    same_path!(interaction::NamedKey);
    same_path!(interaction::PointerButton);
    same_path!(interaction::PointerButtons);
    same_path!(interaction::PointerPress);
    same_path!(interaction::PointerRelease);

    same_path!(platform::SystemPreferences);
    same_path!(platform::MotionPreference);
    same_path!(platform::DurationScale);
    same_path!(platform::TextWeightPreference);
    same_path!(platform::GesturePreferences);
    same_path!(platform::NativeMouseGeometry);
    same_path!(platform::NativeTouchGeometry);
    same_path!(platform::WheelPreferences);
    same_path!(platform::WheelStep);
    same_path!(platform::InvalidPreference);
    same_path!(platform::TargetPlatform);
    same_path!(platform::HapticFeedback);
    same_path!(platform::Brightness);
    same_path!(platform::InvalidLocale);
    same_path!(platform::Availability);
    same_path!(platform::text_store::TextInputPurpose);

    same_path!(painting::Canvas);
    same_path!(painting::SemanticsRegion);
    same_path!(painting::SemanticsRegionKey);
    same_path!(painting::DrawOp);
    same_path!(painting::BlurSigma);
    same_path!(painting::ImagePlacement);
    same_path!(painting::TextSizing);
    same_path!(painting::paint::Path);
    same_path!(painting::styling::Color);
    same_path!(painting::styling::Shadow<f64>);
    same_path!(painting::typography::TextStyle);
    same_path!(painting::typography::TextSpan);
    same_path!(painting::typography::TextDecoration);
    same_path!(painting::typography::TextDecorationLines);
    same_path!(painting::typography::TextDecorationStyle);
    same_path!(painting::typography::DecorationThickness);

    same_path!(rendering::RenderUpdateImpact);
    same_path!(rendering::RenderError);
    same_path!(rendering::RenderResult<()>);
    same_path!(rendering::BoxConstraints);
    same_path!(interaction::HitTestBehavior);
    same_path!(interaction::CursorIcon);
    same_path!(rendering::BoxProtocol);
    same_path!(rendering::SemanticsRole);
    same_path!(rendering::TextDirection);
    same_path!(rendering::ScrollPosition);
}

#[cfg(feature = "testing")]
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod measured_testing {
    use flui_sdk::testing::input::{
        GestureSettingsSource as _, KeyEventBuilder as _, make_down_event_for_id as _,
        make_down_event_for_id_with_button as _, make_move_event_for_id as _,
    };
    use flui_sdk::testing::painting::FontRegistry as _;
    use flui_sdk::testing::rendering::{
        BoxQueryRun as _, DrawCommandSummary as _, DrawKind as _, FrameRun as _, Layer as _,
        LayerTree as _, LayoutRun as _, PaintRun as _, Probe as _, RenderTester as _,
        TreeNode as _, box_node as _, collect_commands as _, render_snapshot as _,
        sliver_node as _,
    };
    use flui_sdk::testing::{
        AlternativesSubject as _, BuildCapabilities as _, DropLedger as _, DropSentinel as _,
        GestureAlternativesCheck as _, HeadlessBinding as _, HeadlessHost as _,
        HeadlessWindow as _, MountOptions as _, MountOwners as _, Mounted as _, PointerScript as _,
        TouchScript as _, a11y as _, assert_gesture_alternatives as _, log_capture as _,
        replay as _, storage as _, widgets as _,
    };
}

/// The common package-test kit is the facade's own API at the same paths. The
/// Evolving additions remain the owning crates' types rather than wrappers.
#[cfg(feature = "testing")]
#[test]
fn the_testing_re_exports_are_the_facades_and_owning_crates_types() {
    same_path!(testing::BuildCapabilities);
    same_path!(testing::HeadlessBinding);
    same_path!(testing::MountOptions);
    same_path!(testing::MountOwners);
    same_path!(testing::Mounted);
    same_path!(testing::DropLedger);
    same_path!(testing::DropSentinel);
    same_path!(testing::a11y::A11yTree);
    same_path!(testing::replay::PointerScript);
    same_path!(testing::widgets::LaidOut);
    same_path!(testing::rendering::RenderTester);

    let _: fn(flui_testing::HeadlessHost) -> flui_sdk::testing::HeadlessHost = |x| x;
    let _: fn(
        flui_interaction::testing::input::KeyEventBuilder,
    ) -> flui_sdk::testing::input::KeyEventBuilder = |x| x;
    let _: fn(
        flui_interaction::gesture::GestureSettingsSource,
    ) -> flui_sdk::testing::input::GestureSettingsSource = |x| x;
    let _: fn(flui_painting::glyphs::FontRegistry) -> flui_sdk::testing::painting::FontRegistry =
        |x| x;
    let _: fn(flui_rendering::layer::LayerTree) -> flui_sdk::testing::rendering::LayerTree = |x| x;
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
    use flui_sdk::platform::input_capabilities::{InputCapabilities as _, PointerKindSet as _};
}

#[cfg(feature = "unstable-input")]
#[test]
fn the_unstable_input_re_exports_are_the_owning_crates_types() {
    let _: fn(
        flui_platform::input_capabilities::InputCapabilities,
    ) -> flui_sdk::platform::input_capabilities::InputCapabilities = |x| x;
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
        "pub mod capability {",
        "pub mod drag {",
        "pub mod haptics {",
        "pub mod input {",
        "pub mod input_capabilities {",
        "pub mod interaction {",
        "pub mod paint {",
        "pub mod painting {",
        "pub mod painting {",
        "pub mod pipeline {",
        "pub mod platform {",
        "pub mod rendering {",
        "pub mod rendering {",
        "pub mod styling {",
        "pub mod testing {",
        "pub mod text_input {",
        "pub mod text_store {",
        "pub mod typography {",
        "pub mod view {",
        "pub use flui_animation as animation;",
        "pub use flui_foundation as foundation;",
        "pub use flui_foundation::geometry;",
        "pub use flui_interaction::ArenaMembership;",
        "pub use flui_interaction::AxisLock;",
        "pub use flui_interaction::BeginContactError;",
        "pub use flui_interaction::CancelOutcome;",
        "pub use flui_interaction::ClipboardBackend;",
        "pub use flui_interaction::ClipboardCapability;",
        "pub use flui_interaction::ClipboardHandle;",
        "pub use flui_interaction::ContactEnd;",
        "pub use flui_interaction::ContactId;",
        "pub use flui_interaction::ContactSnapshot;",
        "pub use flui_interaction::DragDownDetails;",
        "pub use flui_interaction::DragEndDetails;",
        "pub use flui_interaction::DragStartDetails;",
        "pub use flui_interaction::DragUpdateDetails;",
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
        "pub use flui_interaction::MultiTapCount;",
        "pub use flui_interaction::MultiTapCountError;",
        "pub use flui_interaction::ObserverHandle;",
        "pub use flui_interaction::PanZoomEvent;",
        "pub use flui_interaction::PhysicalEdge;",
        "pub use flui_interaction::PointerDispatch;",
        "pub use flui_interaction::PointerEvent;",
        "pub use flui_interaction::PointerEventExt;",
        "pub use flui_interaction::PointerId;",
        "pub use flui_interaction::PointerKind;",
        "pub use flui_interaction::PointerKindSet;",
        "pub use flui_interaction::PrimaryClaim;",
        "pub use flui_interaction::PrimaryClaimId;",
        "pub use flui_interaction::PrimaryClaims;",
        "pub use flui_interaction::PrimaryContact;",
        "pub use flui_interaction::PrimarySelectionBackend;",
        "pub use flui_interaction::PrimarySelectionCapability;",
        "pub use flui_interaction::PrimarySelectionHandle;",
        "pub use flui_interaction::SelectionText;",
        "pub use flui_interaction::SwipeDetails;",
        "pub use flui_interaction::SwipeGestureRecognizer;",
        "pub use flui_interaction::SwipeStart;",
        "pub use flui_interaction::TapDetails;",
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
        "pub use flui_interaction::events::ButtonChange;",
        "pub use flui_interaction::events::Code;",
        "pub use flui_interaction::events::CursorIcon;",
        "pub use flui_interaction::events::Key;",
        "pub use flui_interaction::events::KeyEvent;",
        "pub use flui_interaction::events::KeyState;",
        "pub use flui_interaction::events::NamedKey;",
        "pub use flui_interaction::events::PointerButtons;",
        "pub use flui_interaction::events::make_down_event_for_id;",
        "pub use flui_interaction::events::make_down_event_for_id_with_button;",
        "pub use flui_interaction::events::make_move_event_for_id;",
        "pub use flui_interaction::focus::FocusNode;",
        "pub use flui_interaction::focus::KeyEventResult;",
        "pub use flui_interaction::gesture::CompositionBranch;",
        "pub use flui_interaction::gesture::CompositionError;",
        "pub use flui_interaction::gesture::CompositionKind;",
        "pub use flui_interaction::gesture::ContestCancel;",
        "pub use flui_interaction::gesture::GestureBranches;",
        "pub use flui_interaction::gesture::GestureComposition;",
        "pub use flui_interaction::gesture::GestureContest;",
        "pub use flui_interaction::gesture::GestureSettingsSource;",
        "pub use flui_interaction::gesture::MotionRole;",
        "pub use flui_interaction::gesture::ObservedComposition;",
        "pub use flui_interaction::gesture::ObservedMember;",
        "pub use flui_interaction::gesture::PanClaim;",
        "pub use flui_interaction::gesture::RejectionReason;",
        "pub use flui_interaction::gesture::SequencedBranch;",
        "pub use flui_interaction::ids::DeviceId;",
        "pub use flui_interaction::pointer::EventPropagation;",
        "pub use flui_interaction::pointer::HitTestBehavior;",
        "pub use flui_interaction::testing::input::KeyEventBuilder;",
        "pub use flui_interaction::text_input::CompositionLedger;",
        "pub use flui_interaction::text_input::EditGeneration;",
        "pub use flui_interaction::text_input::LockArbiter;",
        "pub use flui_interaction::text_input::committed_text;",
        "pub use flui_objects::CarouselLayout;",
        "pub use flui_objects::CarouselLayoutError;",
        "pub use flui_objects::CarouselLayoutKind;",
        "pub use flui_objects::CrossAxisAlignment;",
        "pub use flui_objects::FloatingHeaderSnapConfiguration;",
        "pub use flui_objects::FocusRingStyle;",
        "pub use flui_objects::HitShape;",
        "pub use flui_objects::InvalidSliverCrossAxisExtent;",
        "pub use flui_objects::MAX_ITEM_EXTENT;",
        "pub use flui_objects::MainAxisAlignment;",
        "pub use flui_objects::MainAxisSize;",
        "pub use flui_objects::Oval;",
        "pub use flui_objects::OverScrollHeaderStretchConfiguration;",
        "pub use flui_objects::OverflowBoxFit;",
        "pub use flui_objects::PathClipConfiguration;",
        "pub use flui_objects::RenderPhysicalShape;",
        "pub use flui_objects::ShimmerBand;",
        "pub use flui_objects::StackFit;",
        "pub use flui_objects::StretchTriggerSignal;",
        "pub use flui_objects::TableColumnWidth;",
        "pub use flui_objects::TranslationFraction;",
        "pub use flui_objects::WrapAlignment;",
        "pub use flui_objects::WrapCrossAlignment;",
        "pub use flui_painting::Alignment;",
        "pub use flui_painting::BlurSigma;",
        "pub use flui_painting::Canvas;",
        "pub use flui_painting::DrawOp;",
        "pub use flui_painting::ImagePlacement;",
        "pub use flui_painting::TextSizing;",
        "pub use flui_painting::glyphs::FontRegistry;",
        "pub use flui_painting::paint::Clip;",
        "pub use flui_painting::paint::ClipOp;",
        "pub use flui_painting::paint::ImageRepeat;",
        "pub use flui_painting::paint::Morph;",
        "pub use flui_painting::paint::Paint;",
        "pub use flui_painting::paint::Path;",
        "pub use flui_painting::paint::PathCommand;",
        "pub use flui_painting::paint::RoundedPolygon;",
        "pub use flui_painting::paint::StrokeCap;",
        "pub use flui_painting::paint::shapes::material_shapes;",
        "pub use flui_painting::styling::Border;",
        "pub use flui_painting::styling::BorderRadius;",
        "pub use flui_painting::styling::BorderRadiusExt;",
        "pub use flui_painting::styling::BorderSide;",
        "pub use flui_painting::styling::BorderStyle;",
        "pub use flui_painting::styling::BoxDecoration;",
        "pub use flui_painting::styling::BoxShadow;",
        "pub use flui_painting::styling::Color;",
        "pub use flui_painting::styling::Shadow;",
        "pub use flui_painting::styling::TableBorder;",
        "pub use flui_painting::typography::DecorationThickness;",
        "pub use flui_painting::typography::FontWeight;",
        "pub use flui_painting::typography::TextDecoration;",
        "pub use flui_painting::typography::TextDecorationLines;",
        "pub use flui_painting::typography::TextDecorationStyle;",
        "pub use flui_painting::typography::TextDirection;",
        "pub use flui_painting::typography::TextSpan;",
        "pub use flui_painting::typography::TextStyle;",
        "pub use flui_platform::AdmissionError;",
        "pub use flui_platform::Availability;",
        "pub use flui_platform::Brightness;",
        "pub use flui_platform::ClipboardError;",
        "pub use flui_platform::ClipboardRead;",
        "pub use flui_platform::ClipboardWrite;",
        "pub use flui_platform::Concealment;",
        "pub use flui_platform::DurationScale;",
        "pub use flui_platform::EventTime;",
        "pub use flui_platform::GesturePreferences;",
        "pub use flui_platform::HapticFeedback;",
        "pub use flui_platform::HostNotice;",
        "pub use flui_platform::HourCycle;",
        "pub use flui_platform::InvalidLocale;",
        "pub use flui_platform::InvalidPreference;",
        "pub use flui_platform::InvalidRegionalFormat;",
        "pub use flui_platform::Locale;",
        "pub use flui_platform::MeasurementSystem;",
        "pub use flui_platform::MemoryPressureLevel;",
        "pub use flui_platform::MotionPreference;",
        "pub use flui_platform::NativeMouseGeometry;",
        "pub use flui_platform::NativeTouchGeometry;",
        "pub use flui_platform::NumberingSystem;",
        "pub use flui_platform::RegionOverride;",
        "pub use flui_platform::RegionalFormats;",
        "pub use flui_platform::SessionEndBlock;",
        "pub use flui_platform::SessionEndRequest;",
        "pub use flui_platform::SubmitError;",
        "pub use flui_platform::SystemPreferences;",
        "pub use flui_platform::TargetPlatform;",
        "pub use flui_platform::TextWeightPreference;",
        "pub use flui_platform::Unsupported;",
        "pub use flui_platform::UnsupportedReason;",
        "pub use flui_platform::Weekday;",
        "pub use flui_platform::WheelPreferences;",
        "pub use flui_platform::WheelStep;",
        "pub use flui_platform::WindowMode;",
        "pub use flui_platform::WindowState;",
        "pub use flui_platform::capability::Abandoned;",
        "pub use flui_platform::capability::Capability;",
        "pub use flui_platform::capability::CapabilityProvider;",
        "pub use flui_platform::capability::CapabilityRegistrar;",
        "pub use flui_platform::capability::Completion;",
        "pub use flui_platform::capability::CompletionSource;",
        "pub use flui_platform::capability::OwnerPoster;",
        "pub use flui_platform::capability::OwnerRetired;",
        "pub use flui_platform::capability::Plugin;",
        "pub use flui_platform::capability::ProviderContext;",
        "pub use flui_platform::capability::Resolver;",
        "pub use flui_platform::capability::WindowRef;",
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
        "pub use flui_platform::pointer::PointerButton;",
        "pub use flui_platform::pointer::PointerPress;",
        "pub use flui_platform::pointer::PointerRelease;",
        "pub use flui_platform::text_store::TextInputPurpose;",
        "pub use flui_rendering::constraints::BoxConstraints;",
        "pub use flui_rendering::delegates::AspectRatioDelegate;",
        "pub use flui_rendering::delegates::CenterLayoutDelegate;",
        "pub use flui_rendering::delegates::CustomPainter;",
        "pub use flui_rendering::delegates::FlowDelegate;",
        "pub use flui_rendering::delegates::FlowPaintingContext;",
        "pub use flui_rendering::delegates::MultiChildLayoutContext;",
        "pub use flui_rendering::delegates::MultiChildLayoutDelegate;",
        "pub use flui_rendering::delegates::SingleChildLayoutDelegate;",
        "pub use flui_rendering::delegates::SliverGridDelegate;",
        "pub use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;",
        "pub use flui_rendering::delegates::SliverGridDelegateWithMaxCrossAxisExtent;",
        "pub use flui_rendering::delegates::SliverGridLayout;",
        "pub use flui_rendering::layer::{Layer, LayerTree};",
        "pub use flui_rendering::parent_data::{FlexFit, TableCellVerticalAlignment};",
        "pub use flui_rendering::protocol::BoxProtocol;",
        "pub use flui_rendering::semantics::ActionCall;",
        "pub use flui_rendering::semantics::AnnounceError;",
        "pub use flui_rendering::semantics::Announcer;",
        "pub use flui_rendering::semantics::AutoComplete;",
        "pub use flui_rendering::semantics::CellPosition;",
        "pub use flui_rendering::semantics::Current;",
        "pub use flui_rendering::semantics::HasPopup;",
        "pub use flui_rendering::semantics::HeadingLevel;",
        "pub use flui_rendering::semantics::Invalid;",
        "pub use flui_rendering::semantics::LiveSetting;",
        "pub use flui_rendering::semantics::NumericRange;",
        "pub use flui_rendering::semantics::NumericRangeError;",
        "pub use flui_rendering::semantics::Reveal;",
        "pub use flui_rendering::semantics::ScrollUnit;",
        "pub use flui_rendering::semantics::SemanticsAnchor;",
        "pub use flui_rendering::semantics::SemanticsConfiguration;",
        "pub use flui_rendering::semantics::SemanticsRegion;",
        "pub use flui_rendering::semantics::SemanticsRegionKey;",
        "pub use flui_rendering::semantics::SemanticsRole;",
        "pub use flui_rendering::semantics::SetSize;",
        "pub use flui_rendering::semantics::SortDirection;",
        "pub use flui_rendering::semantics::TableShape;",
        "pub use flui_rendering::semantics::TextDirection;",
        "pub use flui_rendering::semantics::TextSelection;",
        "pub use flui_rendering::semantics::Toggled;",
        "pub use flui_rendering::semantics::Urgency;",
        "pub use flui_rendering::testing::inspect::render_snapshot;",
        "pub use flui_rendering::testing::{",
        "pub use flui_rendering::view::ScrollDirection;",
        "pub use flui_rendering::view::ScrollPosition;",
        "pub use flui_rendering::{RenderError, RenderResult, RenderUpdateImpact};",
        "pub use flui_testing::widgets;",
        "pub use flui_testing::{",
        "pub use flui_testing::{DropLedger, DropSentinel};",
        "pub use flui_testing::{a11y, log_capture, replay, storage};",
        "pub use flui_view::*;",
        "pub use flui_widgets as widgets;",
    ];
    assert_eq!(declared_surface(), pinned);
}
