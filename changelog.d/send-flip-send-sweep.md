### Changed

- Curve setters (`implicitly_animated` builders, `Effects::curve`, `AnimatedSwitcher::switch_in_curve`/`switch_out_curve`, `Transition::curve`, `Hero::curve`/`reverse_curve`, `Keyframes`, `MotionSpec::with_curve`, `OpenContainer::curve`) take `impl Curve + 'static` instead of `impl Curve + Send + Sync + 'static`. Migration: none; a curve holding `Rc` state is now accepted.
- `ArcCurve` is renamed `SharedCurve` and holds a custom curve behind `Rc`, so it is no longer `Send`. Migration: rename `ArcCurve` to `SharedCurve`.
- `ScrollPhysics` drops its `Send + Sync` supertrait, and `SharedScrollPhysics` is `Rc<dyn ScrollPhysics>`. Migration: `.physics(Arc::new(p))` becomes `.physics(Rc::new(p))`.
- `SemanticsActionHandler` is `Rc<dyn Fn(ActionCall)>` (was `Arc<dyn Fn(ActionCall) + Send + Sync>`), so `SemanticsConfiguration` is no longer `Send`. Migration: `Arc::new(handler)` becomes `Rc::new(handler)`.
- `LifecycleStateCallback` is `Rc<dyn Fn(AppLifecycleState)>`, the type `UpdateScheduler::add_lifecycle_state_listener` takes.
- `RenderObject::metadata`, `RenderBox::metadata`, `RenderSliver::metadata` and `HitTestEntry::metadata` carry an owner-local `HitMetadata` (`HitMetadata::new(value)`, `downcast_ref`, `ptr_eq`) instead of `Arc<dyn Any + Send + Sync>`; `HitTestEntry` and `HitTestResult` are no longer `Send`. `RenderMetaData` and `MetaData::new` accept any `'static` value. Migration: `Some(Arc::new(v))` becomes `Some(HitMetadata::new(v))`; `MetaData::shared` takes a `HitMetadata`.
- `CustomPaint::painter`/`foreground_painter`, `Flow::new`, `CustomSingleChildLayout::new`, `CustomMultiChildLayout::new`, `GridView::builder`, `AnimatedGrid::new`, `SliverGrid::new`/`list` and `RenderSliverGrid::new` take the painter or delegate by value (`impl Trait + 'static`). Migration: `.painter(Rc::new(p))` becomes `.painter(p)`, and `Flow::new(Rc::new(d), ..)` becomes `Flow::new(d, ..)`. `RenderSliverGrid::with_shared_delegate`, `RenderSliverGrid::set_grid_delegate` and `SliverGrid::over` take a shared `Rc<dyn SliverGridDelegate>`.
- `WidgetStateProperty::resolve_with` accepts a closure without `Send + Sync`; `WidgetStateProperty` and `WidgetStatesController` are no longer `Send`.
- `CupertinoTabController` and `PageView` keep their state in `Rc<Cell>`/`Rc<RefCell>` instead of atomics and mutexes.

### Removed

- The unused `flui_foundation` callback aliases `VoidCallback`, `ValueChanged`, `ValueGetter`, `ValueSetter`, `Predicate`, `ValueTransformer` and `FallibleCallback`. Migration: write the closure type in place.
- The `MetaDataPayload` alias. Migration: use `HitMetadata`.
