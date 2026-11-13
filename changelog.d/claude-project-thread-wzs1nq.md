### Added

- `flui::rendering::objects`: the built-in render objects (`RenderAlign`, `RenderFlex`,
  `RenderSliverList`, …) and the configuration types they take, as Flutter's rendering library
  exports them. Each built-in widget's `RenderView::RenderObject` is now a path an author can write.
- `flui::rendering` re-exports the render-contract types its own items name: the `Protocol`
  associated types (`BoxLayout`, `SliverLayout`, `ProtocolConstraints`, `ChildState`, …),
  `LayoutContext`, `HitTestContext`, `PaintAnchor`, `PaintFragment`, `FragmentRecorder`,
  `GrowthDirection`, `InputRegionSink`, `SemanticsClip`, `PoisonPhase`, `DirtySendError` and the
  parent-data types a sliver cross axis takes. Each was reachable only by an internal crate path.
- `flui::semantics`: the vocabulary `SemanticsConfiguration` takes, as Flutter's semantics
  library exports it: tags, sort keys, hint overrides, relations, collections, states, text
  content, custom actions and their handlers, announcements. Each was reachable only by an internal
  crate path. `flui::painting` adds `SemanticsRegionId` beside the region types that name it.

### Removed

- `RawId::zip_unchecked`, `Id::zip_unchecked` and `Id::new_unchecked`, the hidden `unsafe`
  constructors of `flui::foundation` ids; the safe constructors stay.
- `flui_sdk::pipeline::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction}`: they are
  `flui_sdk::rendering::objects::*`, at the facade's path.

### Changed

- devtools' `Verdict` and `EventCategory` are `#[non_exhaustive]`, so a `match` on one needs a `_`
  arm (ADR-0329).
- These public enums are `#[non_exhaustive]` too (ADR-0329): `SpeedLimit`, `StartupWindow`,
  `DiagnosticsProfile`, `CacheCapacity`, `AssetError`, `LoadState`, `CupertinoButtonSize`,
  `SnackBarClosedReason` and `TabIndicatorAnimation`.
- Also `#[non_exhaustive]` (ADR-0329): the scheduler's `BudgetPolicy`, `SchedulerPhase`,
  `FramePhase` and `Priority`; the layer crate's `DamageMode`, `PlatformViewHitTestBehavior`,
  `SceneError`, `FullReason` and `ResyncReason`; flui-log's `EventOrigin`, `SubscriberPolicy`,
  `InstallPolicy` and `LogBridgePolicy`; and flui-hot-reload's `ReloadStrategy`, `HotReloadTier`,
  `HotReloadOutcome`, `PluginKind` and `WorkerPollOutcome`.
- Also `#[non_exhaustive]` (ADR-0329): `MainAxisAlignment`, `CrossAxisAlignment`, `StackFit`,
  `OverflowBoxFit`, `TableColumnWidth`, `WrapAlignment`, `WrapCrossAlignment`, `MinTarget`,
  `ViewFocusDirection`, `KeyLookup`, `GeometryError`, the inspect `Change`, `FieldError`,
  `MatchError`, `Link` and `Role`, and the testing kit's `BuildCapabilities`, `PointerPhase`,
  `A11yQuery`, `A11yQueryError`, `InvokeActionError`, `RowOutcome`, `StoreHostCall` and
  `HandlerCall`.
- Also `#[non_exhaustive]` (ADR-0329): the semantics `AccessibilityFocusBlockType`,
  `DebugSemanticsDumpOrder`, `StringAttributeType` and `SemanticsCollectionOrigin`, and the runtime's
  `Privacy`, `ReloadTier`, `CommandSendError` and `UiCommand`.
- Also `#[non_exhaustive]` (ADR-0329): the rendering `UsageByParent`, `SemanticsClip`, `DrawKind`
  and `ParentDataSeed`, and the engine's raster-executor commands, slot states, health, failures and
  errors (`RasterCommand`, `SlotTarget`, `SlotChange`, `SlotVisibility`, `SlotPhase`, `SlotFailure`,
  `ExecutorHealth`, `DeviceHealth`, `RasterSendError`, `RasterSubmitError`, `FrameDropReason`) and
  its testing enums.
- Also `#[non_exhaustive]` (ADR-0329): the painting `TextAlign`, `TextAlignVertical`, `TextOverflow`,
  `TextWidthBasis`, `TextLeadingDistribution`, `PlaceholderAlignment`, `FontStyle`, `FilterQuality`,
  `ImageRepeat`, `ClipBehavior`, `BorderStyle`, `BorderPosition`, `PhysicalShape`, `MaterialType`,
  `ShadowQuality`, `BlurQuality`, `BlurMode`, `ColorAdjustment`, `PathPaintMode`, `FaceRef` and
  the painting error enums.
- Also `#[non_exhaustive]` (ADR-0329): the interaction `DragRepresentation`, `DragStage`,
  `MoveOutcome`, `Autoscroll`, `TraversalEdgeBehavior` and `SweepModel`.
- Also `#[non_exhaustive]` (ADR-0329): the widgets `TransitionGroup`, `RouteLifecycle`, `PanAxis`,
  `WheelScaleGate` and `DismissDirection`.
- `flui-protocol` decides each public enum (ADR-0329): the recording mirrors, `RecordRow`,
  `Checked`, `Retry` and `RequestId` stay closed with their reason at the site.
- `flui-native` decides each public enum (ADR-0329), so every crate of the workspace now states
  each public enum's closure at its declaration; `MacOSWindowLevel`, `AxisSource`, `ClockChoice`,
  `CodecError`, `ReplayError` and `AppCall` are `#[non_exhaustive]`.
- `flui::app` is a curated module instead of the whole `flui-app` crate: the entry, its
  configuration (`FrameLatency` included), background work, close requests, frame failures, window
  control and the input tap. `WidgetsBinding`, `RenderingBinding`, the root render view,
  `embedder`, `native`, `testing`, `prelude` and the view re-exports are no longer under
  `flui::app`. The types public signatures name moved: `PipelineOwner` and `PipelineCell` to
  `flui::rendering`, `UpdateScheduler` to `flui::view`, `GestureBinding` to
  `flui::testing::harness`.
