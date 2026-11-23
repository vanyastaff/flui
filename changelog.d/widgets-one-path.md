### Changed

- **`flui-widgets`**: every widget item has one public path, the crate root: `flui::widgets::X`
  (`flui_widgets::X`, `flui_sdk::widgets::X`). The particle effects stay a namespace of their
  own, `flui::widgets::particles` (`ParticleEmitter`, `ParticleConfig`, `ParticleController`
  and their vocabulary), which the root does not also flatten.
- **`flui`** and **`flui-sdk`**: the types a widget takes from another crate are reached from
  that crate's module. The facade adds `flui::rendering::{AspectRatioDelegate,
  CenterLayoutDelegate, FlowDelegate, FlowPaintingContext, MultiChildLayoutContext,
  MultiChildLayoutDelegate, SingleChildLayoutDelegate, SliverGridDelegate,
  SliverGridDelegateWithFixedCrossAxisCount, SliverGridDelegateWithMaxCrossAxisExtent,
  SliverGridLayout, ScrollPosition}`, `flui::painting::ImagePlacement` and
  `flui::interaction::EventPropagation`; the SDK adds a `semantics` module and the
  `interaction`, `painting`, `rendering` and `rendering::objects` items its packages name.
  `flui::widgets::prelude` keeps importing the ones an app author needs.

### Removed

- **`flui-widgets`**: the module paths (`flui::widgets::layout::Padding`,
  `flui::widgets::scroll::…`, `flui::widgets::text::…`, `flui::widgets::motion::particles`,
  `flui::widgets::router::…` and the other 20 top-level modules); use the root path.
- **`flui-widgets`**: the re-exports of other crates' types at the widgets root. Reach them
  from their owners:
  - semantics roles, states, ranges and configuration (`SemanticsRole`, `Toggled`,
    `SemanticsConfiguration`, `NumericRange`, `HeadingLevel`, `LiveSetting`, …; the former
    `SemanticsScrollUnit` is `ScrollUnit`): `flui::semantics`;
  - pointer events and gesture details (`PointerEvent`, `ButtonChange`, `PointerDispatch`,
    `EventPropagation`, `DeviceId`, `PanZoomEvent`, `CursorIcon`, `TapDetails`, the
    `Drag*Details`, `GestureEndReason`, `MultiTapCount`): `flui::interaction`; `Cursor` and
    `CursorImage`: `flui::platform`;
  - `HitTestBehavior`, `FlexFit`, `TableCellVerticalAlignment`, `ScrollPosition` and the layout
    delegates: `flui::rendering`; `CustomPainter`: `flui::painting`;
  - the layout and configuration enums of the render objects (`MainAxisAlignment`,
    `CrossAxisAlignment`, `MainAxisSize`, `StackFit`, `VerticalDirection`, `WrapAlignment`,
    `WrapCrossAlignment`, `OverflowBoxFit`, `HitShape`, `TableColumnWidth`,
    `TranslationFraction`, `Oval`, `ShimmerBand`, `FocusRingStyle`, `CarouselLayout` and its
    error and kind, `MAX_ITEM_EXTENT`, `InvalidSliverCrossAxisExtent`,
    `FloatingHeaderSnapConfiguration`, `OverScrollHeaderStretchConfiguration`,
    `StretchTrigger`): `flui::rendering::objects`;
  - `ImagePlacement`: `flui::painting`; `ImageRepeat`: `flui::painting::paint`;
    `TableBorder`: `flui::painting::styling`; `Brightness`: `flui::platform`;
  - `HapticsMode`: `flui::view`; `SliverPersistentHeaderDelegate`: `flui::view::element`;
    `RoutePath`, `RouteParseError`, `NavigationIntent`, `NavigationOp` and
    `NavigationRefused`: `flui::view::navigation`; `AnimationRunFuture` and `RunCanceled`:
    `flui::animation`.
