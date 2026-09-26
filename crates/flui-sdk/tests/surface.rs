//! The SDK's surface is the measured list, and each re-export is the item it
//! names, not a copy.
//!
//! The list is every path `flui-material` and `flui-cupertino` import from the
//! internal crates outside their tests, rewritten to its SDK path (an
//! associated item reduced to its type), plus `painting::DrawOp`, which their
//! paint tests read. ARCHITECTURE.md records how it was measured.

/// Every measured item through its SDK path: removing or moving one fails to
/// build this test.
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod measured {
    use flui_sdk::animation::ext::{AnimatableExt as _, AnimationExt as _};
    use flui_sdk::animation::{
        Animation as _, AnimationController as _, AnimationStatus as _, ArcCurve as _,
        ConstantAnimation as _, Curve as _, CurvedAnimation as _, Curves as _, FloatTween as _,
        TickerFuture as _, Tween as _, UpdateScheduler as _, Vsync as _, VsyncRegistration as _,
        animate as _,
    };
    use flui_sdk::foundation::notifier::Listenable as _;
    use flui_sdk::foundation::{
        ChangeNotifier as _, ElementId as _, Listenable as _, ListenerCallback as _,
        ListenerId as _, ViewKey as _,
    };
    use flui_sdk::interaction::{DragDownDetails as _, FocusNode as _};
    use flui_sdk::painting::{Canvas as _, DrawOp as _};
    use flui_sdk::pipeline::{
        PathClipConfiguration as _, RenderPhysicalShape as _, TranslationFraction as _,
    };
    use flui_sdk::rendering::{
        BoxConstraints as _, BoxProtocol as _, HitTestBehavior as _, RenderUpdateImpact as _,
    };
    use flui_sdk::types::geometry::{
        EdgeInsets as _, Pixels as _, RRect as _, Radius as _, px as _,
    };
    use flui_sdk::types::layout::Alignment as _;
    use flui_sdk::types::painting::{Clip as _, Paint as _, Path as _};
    use flui_sdk::types::platform::{Brightness as _, Locale as _};
    use flui_sdk::types::styling::{
        Border as _, BorderRadius as _, BorderRadiusExt as _, BorderSide as _, BorderStyle as _,
        BoxDecoration as _, Color as _,
    };
    use flui_sdk::types::typography::{FontWeight as _, TextDirection as _, TextStyle as _};
    use flui_sdk::types::{
        Alignment as _, Color as _, EdgeInsets as _, Offset as _, Pixels as _, Point as _,
        RRect as _, Rect as _, Size as _,
    };
    use flui_sdk::view::element::ElementKind as _;
    use flui_sdk::view::prelude::{BuildContext as _, InheritedData as _, StatelessView as _};
    use flui_sdk::view::{
        AnimatedView as _, BoxedView as _, BuildContext as _, BuildContextExt as _, Child as _,
        FieldMask as _, GlobalKey as _, InheritedData as _, InheritedView as _, IntoView as _,
        LocalPostFrameHandle as _, RebuildHandle as _, RebuildReason as _,
        RenderObjectContext as _, RenderView as _, StatefulView as _, View as _, ViewExt as _,
        ViewState as _, impl_animated_view as _, impl_inherited_view as _, impl_render_view as _,
        single_child_view_children as _,
    };
    use flui_sdk::widgets::animated::VsyncScope as _;
    use flui_sdk::widgets::icon::IconData as _;
    use flui_sdk::widgets::layout::PreferredSizeView as _;
    use flui_sdk::widgets::prelude::BoxConstraints as _;
    use flui_sdk::widgets::{
        Actions as _, ActivateIntent as _, Align as _, AnimatedBuilder as _, AppBuilder as _,
        BoxedLocalizationsDelegate as _, ButtonActivateIntent as _, CallbackAction as _,
        Center as _, ClipRect as _, ColoredBox as _, Column as _, ConstrainedBox as _,
        Container as _, CrossAxisAlignment as _, CustomMultiChildLayout as _, CustomPaint as _,
        CustomPainter as _, DecoratedBox as _, DefaultTextStyle as _, Directionality as _,
        EditableText as _, Expanded as _, FadeTransition as _, Flexible as _,
        FloatingHeaderSnapConfiguration as _, Focus as _, GestureDetector as _, HeroMode as _,
        HitTestBehavior as _, Icon as _, IconData as _, IconTheme as _, IconThemeData as _,
        InheritedTheme as _, IntrinsicWidth as _, LayoutId as _, MainAxisAlignment as _,
        MainAxisSize as _, MediaQuery as _, MediaQueryData as _, MergeSemantics as _,
        MouseRegion as _, MultiChildLayoutContext as _, MultiChildLayoutDelegate as _,
        NavigatorHandle as _, NavigatorObserver as _, Offstage as _, Opacity as _, Padding as _,
        PageRoute as _, PopupRoute as _, Positioned as _, PreferredSizeView as _,
        RouteAnimation as _, RouteResult as _, Row as _, SafeArea as _, Semantics as _,
        SemanticsRole as _, SizedBox as _, SlideTransition as _, SliverPersistentHeader as _,
        SliverPersistentHeaderDelegate as _, Stack as _, StackFit as _, SubmitCallback as _,
        Table as _, TableCell as _, TableCellVerticalAlignment as _, TableColumnWidth as _,
        TableRow as _, Text as _, TextEditingController as _, TickerMode as _, Transform as _,
        WidgetState as _, WidgetStateConstraint as _, WidgetStateProperty as _, WidgetStates as _,
        WidgetStatesController as _, WidgetsApp as _,
    };
}

/// Each curated item, and one type per whole-module re-export, is the facade's
/// own type: this fails to build if an SDK path becomes a wrapper, a newtype
/// or a different item of the same name.
#[test]
fn the_re_exports_are_the_facades_types() {
    let _: fn(flui::animation::AnimationController) -> flui_sdk::animation::AnimationController =
        |x| x;
    let _: fn(flui::foundation::ElementId) -> flui_sdk::foundation::ElementId = |x| x;
    let _: fn(flui::types::Color) -> flui_sdk::types::Color = |x| x;
    let _: fn(flui::view::RebuildHandle) -> flui_sdk::view::RebuildHandle = |x| x;
    let _: fn(flui::widgets::Text) -> flui_sdk::widgets::Text = |x| x;

    let _: fn(flui::interaction::DragDownDetails) -> flui_sdk::interaction::DragDownDetails = |x| x;
    let _: fn(flui::interaction::FocusNode) -> flui_sdk::interaction::FocusNode = |x| x;
    let _: fn(flui::painting::Canvas) -> flui_sdk::painting::Canvas = |x| x;
    let _: fn(flui::painting::DrawOp) -> flui_sdk::painting::DrawOp = |x| x;
    let _: fn(flui::rendering::RenderUpdateImpact) -> flui_sdk::rendering::RenderUpdateImpact =
        |x| x;
    let _: fn(flui::rendering::BoxConstraints) -> flui_sdk::rendering::BoxConstraints = |x| x;
    let _: fn(flui::rendering::HitTestBehavior) -> flui_sdk::rendering::HitTestBehavior = |x| x;
    let _: fn(flui::rendering::BoxProtocol) -> flui_sdk::rendering::BoxProtocol = |x| x;
}

/// The `pub use` and `pub mod` lines of `src/lib.rs`, trimmed and sorted.
fn declared_surface() -> Vec<&'static str> {
    let mut lines: Vec<&str> = include_str!("../src/lib.rs")
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub use ") || line.starts_with("pub mod "))
        .collect();
    lines.sort_unstable();
    lines
}

/// Adding an item to the SDK is a decision (ADR-0088 §4 caps the Evolving
/// surface), so it changes this list in the same change.
#[test]
fn the_public_surface_is_the_measured_list() {
    let pinned = [
        "pub mod interaction {",
        "pub mod painting {",
        "pub mod pipeline {",
        "pub mod rendering {",
        "pub use flui_animation as animation;",
        "pub use flui_foundation as foundation;",
        "pub use flui_interaction::DragDownDetails;",
        "pub use flui_interaction::routing::FocusNode;",
        "pub use flui_objects::PathClipConfiguration;",
        "pub use flui_objects::RenderPhysicalShape;",
        "pub use flui_objects::TranslationFraction;",
        "pub use flui_painting::Canvas;",
        "pub use flui_painting::DrawOp;",
        "pub use flui_rendering::RenderUpdateImpact;",
        "pub use flui_rendering::constraints::BoxConstraints;",
        "pub use flui_rendering::hit_testing::HitTestBehavior;",
        "pub use flui_rendering::protocol::BoxProtocol;",
        "pub use flui_types as types;",
        "pub use flui_view as view;",
        "pub use flui_widgets as widgets;",
    ];
    assert_eq!(declared_surface(), pinned);
}
