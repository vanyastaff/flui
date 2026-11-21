### Changed

- `flui-view` names each item at one path. Its closed modules (`binding`, `child`, `context`,
  `key`, `owner`, `reactive`, `seq`, `tree`, `view`) are private: `flui_view::seq::ViewSeq`,
  `flui_view::owner::KeepAliveLease`, `flui_view::context::LifecycleContext`,
  `flui_view::view::ViewExt`, `flui_view::reactive::Reactive` and the rest are
  `flui_view::ViewSeq`, `flui_view::KeepAliveLease`, … (and so under `flui::view` and
  `flui_sdk::view`). `flui::view::key::ValueKey`, `GlobalKey` and `ObjectKey` are
  `flui::view::ValueKey`, …; `UniqueKey` and `ViewKey` are `flui::foundation`'s.
- The items of the open module `element` are named only through it: `Lifecycle`, `ElementSlot`,
  `IndexedSlot`, `ProvidedRef`, `NotifiableElement`, `StatefulBehavior`, `StatelessBehavior`
  and the element aliases (`StatelessElement`, `StatefulElement`, `InheritedElement`,
  `ProxyElement`, `RenderElement`, `AnimatedElement`, `ParentDataElement`) left the root. The
  notifications, `Provided` and `ProvidedValue` are at the root only. The reconciler's
  `ReconcileEvent`, `ReconcileEventKind` and `RECONCILE_TARGET` are at the root, and its test
  collector (`ReconcileEventCollector`, `CollectedEvent`, `reconcile_children`) is in
  `flui_view::testing`.
- The owners only a composition root builds left the public surface for the hidden runtime seam:
  `WidgetsBinding`, `AttachError`, `BuildOwner`, `BuildPremise`, `BuildInProgress`,
  `CapabilitySource`, `PresentationCapabilities`, `ElementTree`, `ElementNode`,
  `FrameBuildReport`, `GlobalKeyScope`, `DuplicateGlobalKey`, `LifecycleHook`, `RecoveredAt`,
  `RecoveredPanic`, `RootElement`, `RootElementImpl`, `RenderTreeRootElement`,
  `RootRenderElement`, `RootRenderView` and `SlotInfo`. `flui::view::WidgetsBinding` is gone;
  `LifecycleHook` and `RecoveredAt` stay at `flui::app`.
- `flui-view` re-exports no other crate's ids or scheduler handles: `ElementId`, `RenderId`,
  `CreationSite` and `RebuildReasons` are `flui::foundation`'s, `RenderUpdateImpact` is
  `flui::rendering`'s, and `ReadGraph`, `ReaderSink` and `ScopeRef` are
  `flui_foundation::read_scope`'s. The scheduler's `AsyncDriver`, `BoxedTask`, `FrameId`,
  `FramePhase`, `FrameTiming`, `FrameWaker`, `Instant`, `PostFrameHandle`,
  `PostFrameScheduleError`, `Sleep`, `TaskToken` and `TimerHandle` are at the new
  `flui::scheduler`, and the ones packages hold (`AsyncDriver`, `FrameWaker`, `PostFrameHandle`,
  `TaskToken`) at `flui_sdk::scheduler`; `flui::view::UpdateScheduler` is
  `flui::scheduler::UpdateScheduler`.
- `flui_view::prelude` (and with it `flui::prelude` and `flui_widgets::prelude`) holds the
  authoring forms only: it no longer exports `tracing`'s `debug!`, `error!`, `info!`, `trace!`
  and `warn!`, `BuildOwner`, `ElementTree`, `ElementNode`, `ElementOwner`, `WidgetsBinding`,
  `RebuildReasons`, `RootElement`, `Lifecycle`, `ElementSlot`, `IndexedSlot`,
  `NotifiableElement`, `ElementId`, `RenderId`, `CreationSite` or `RenderUpdateImpact`.
