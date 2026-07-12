<!-- Rust Code Studio task breakdown for ADR-0027 interaction routing. -->

# Tasks: ADR-0027 owner-routed interaction routes

- **Spec:** [`spec.md`](spec.md)   ·   **Updated:** `2026-07-12`

## Task list

*Ordered. Each task is small enough for one /dev-task, except the explicitly atomic
cross-crate migrations whose intermediate states would violate the architecture or
Flutter semantics. This file is the durable source of truth. Status: ☐ todo ·
◐ in-progress · ☑ done · ⊘ blocked.*

| # | Task outcome | Acceptance slice | Owner lead | Blocked by | Status |
|---|--------------|------------------|------------|------------|--------|
| 1 | Land the inert `InteractionLane`, non-ABA IDs, owner-local registry, resolved-route storage, and typed errors without changing production dispatch. | Lane/gesture/cell/route auto-traits; `WrongThread`, `InactiveRealm`, `WrongRealm`, `OwnerGone`, `TargetGone`, and `StaleRoute`; borrows end before invoke/drop; stale identities cannot alias reused slots or realms. | `systems-perf-lead` | — | ☑ done |
| 2 | Give `UiRealm` and `HeadlessBinding` one interaction lane, activate it with owner scopes, add the narrow `RenderObjectContext`, and eliminate the test harness's double binding. | LIFO/unwind-safe activation; wrong-realm and two-realm isolation; create/update can register and replace handlers; headless pointer helpers use the same binding/lane as the mounted tree. | `chief-architect` | 1 | ☑ done |
| 3 | Atomically migrate ordinary pointer delivery to data-only targets and one resolver/invoker used by cached and direct dispatch. | Every-target leaf-first synchronous delivery with local transforms and no `EventPropagation::Stop`; Down resolves/caches before arena close; Move reuses; Up/Cancel delivers before sweep/release; unmount/rebuild lifetime rules; per-target panic continuation and cleanup before unwind. | `api-design-lead` | 2 | ☑ done |
| 4 | Atomically owner-localize `GestureBinding`, arena members, recognizers, and gesture callback storage, including the minimum View/State bound ripple. | No gesture callback remains behind `Arc<Mutex<_>>` or `Send + Sync`; no gesture-detail token/queue bridge; long-press, double-tap, tap, and pan retain Flutter ordering through real `HeadlessBinding`; render data remains `Send + Sync`. | `systems-perf-lead` | 3 | ◐ in-progress |
| 5 | Atomically migrate `MouseRegion` and mouse tracking to data-only targets with owner-local strong annotations. | Enter/hover/exit are local callbacks; previous annotations survive long enough to emit Exit after removal; new/current annotation diffing and panic/drop behavior are covered without executable closures in render storage. | `api-design-lead` | 4 | ☑ done |
| 6 | Complete ADR-0027 step 2c with a realm-local `NavigatorHandle` and `UiCommandSender` as the only cross-thread navigation ingress. | Navigation mutations are owner-thread capabilities; no generic UI-thread executor or cross-thread closure API; wrong/dead realm behavior is typed and tested. | `api-design-lead` | 2 | ☑ done |
| 7 | Finish the workspace View/State/callback bound wave and prove local authoring with `Rc<Cell<_>>` while preserving the Send data plane. | Public `Listener`, gesture, mouse, and navigation authoring accepts owner-local captures; render objects, hit entries, targets, route tokens, scene/frame data, and approved cross-thread commands retain required auto-traits. | `api-design-lead` | 4, 5, 6 | ◐ in-progress |
| 8 | Verify Flutter parity, destruction/reentrancy safety, public API shape, and measured routing cost across the completed slice. | Cached/direct parity; unmount/rebuild/stale-route/realm teardown cases; `DropProbe` owner-thread/outside-borrow coverage; nested/reentrant and panic cleanup; Criterion 1/4/16-target baselines and common-Move allocation evidence; semver/API review. | `qa-lead` | 7 | ☐ todo |
| 9 | Reconcile ADR-0027 and project documentation with the implementation, run final gates, and complete `/spec-verify`. | ADR status and consequences match shipped ownership; architecture/API docs contain no obsolete closure path; format, inventory, port-check, clippy, tests, doctests, docs, and applicable platform/render gates pass with evidence. | `chief-architect` | 8 | ☐ todo |

## Critical path

`1 → 2 → 3 → 4 → 5 → 7 → 8 → 9`

The navigation work is a branch, `2 → 6 → 7`: it can proceed after realm/context
ownership lands, but the final bound wave cannot close until both the interaction path
and navigation ownership contract are complete.

## Cross-crate ripples

- **Task 1:** `flui-interaction` gains the lane, IDs, registry, routes, errors, and
  auto-trait tests; downstream crates must not consume the inert surface yet.
- **Task 2:** `flui-app`, `flui-binding`, `flui-view`, and widget test support gain
  lane ownership/activation and `RenderObjectContext`; existing post-frame owner scopes
  must remain intact.
- **Task 3:** `flui-interaction`, `flui-rendering`, `flui-objects`, `flui-view`,
  `flui-widgets`, binding callers, and integration tests change together because
  `PointerEventHandler`, `HitTestEntry`, and ordinary propagation are public seams.
- **Task 4:** recognizers, arena storage, `GestureBinding`, widgets, View/State bounds,
  and headless gesture tests move together; `.flutter/` is the behavioral reference for
  dispatch/arena ordering.
- **Task 5:** `flui-interaction`, `flui-objects`, `flui-widgets`, mouse-tracker users,
  and tests move together so previous annotation lifetime is never temporarily lost.
- **Task 6:** navigation surfaces in `flui-app`/widgets and command ingress in the
  embedding/runtime layer must agree on the realm-local versus cross-thread boundary.
- **Task 7:** the remaining `flui-view`, widgets, app/binding, and example/compiler
  fallout is one workspace-wide API-bound audit; it must not weaken data-plane traits.
- **Tasks 8–9:** public API/semver tooling, benchmarks, Flutter parity harnesses, ADRs,
  architecture docs, examples, and CI inventories must be updated from the final shape.

## Notes

| Task | Size | Required sign-off |
|------|------|-------------------|
| 1 | L | `chief-architect`, `api-design-lead` |
| 2 | XL | `api-design-lead`, `qa-lead` |
| 3 | XL | `chief-architect`, `qa-lead` |
| 4 | XL | `chief-architect`, `api-design-lead`, `qa-lead` |
| 5 | L | `qa-lead`, `chief-architect` |
| 6 | L | `chief-architect`, `qa-lead` |
| 7 | XL | `chief-architect`, `qa-lead` |
| 8 | L | `systems-perf-lead`, maintainer reviewer |
| 9 | M | `docs-engineer`, release/gate reviewer |

- Tasks 3, 4, and 5 are intentionally atomic despite their size. Splitting any of them
  would land a data-only entry without a resolver, a mixed local/`Send` gesture graph,
  or broken mouse Exit lifetime. Each still runs through one `/dev-task` with internal
  RED → GREEN → REFACTOR checkpoints and a single coherent landing.
- This worktree already contains overlapping ADR-0027 changes. Implementation writes
  must be serialized, scoped to the active task, and reviewed against the pre-existing
  diff before every patch; do not reset, stash, or rewrite unrelated user changes.
- 2026-07-12 Task 4 checkpoint: `flui-interaction` recognizer callback storage has
  been owner-localized for tap/double-tap/long-press/drag/multi-tap/tap-and-drag/
  scale/force-press, with a regression proving `Rc<Cell<_>>` works in a tap callback.
  `cargo check -p flui-interaction --tests`, `cargo fmt --package flui-interaction
  -- --check`, and `cargo test -p flui-interaction
  tap_callback_accepts_owner_local_rc_state` pass. Follow-up owner-local fallout in
  `flui-view` removed `WidgetsBindingObserver: Send + Sync`, updated the stale
  `BoxedView`/`BoxedElement`/`ElementTree`/`BuildContext` thread-safety assertions,
  and kept `BuildOwner`'s frame-request hook as the `Send + Sync` data-plane wake
  rather than letting it capture the owner-local `WidgetsBinding`; `cargo check -p
  flui-view --tests` and `cargo fmt --package flui-view -- --check` pass. The
  `RouteBinding::wake` callback is now owner-local too, because it reaches the
  owning `NavigatorShared`; `cargo fmt --package flui-widgets -- --check` passes.
  The remaining `flui-widgets --lib` frontier is down to the two animation-listener
  seams (`HeroFlight::proxy.add_listener` and
  `TransitionRoute::controller.add_status_listener`). That is not a reason to
  restore `Send + Sync` to UI callbacks: it is the Task 6/7 `Navigator`/animation
  owner-lane bridge seam, where render-safe `Animation/Listenable` data must remain
  `Send + Sync` while route/widget actions execute on the owner lane.
- 2026-07-12 follow-up checkpoint: the two `flui-widgets --lib` animation-listener
  seams are now bridged without weakening the data plane. `HeroFlight` no longer
  captures `FlightInner`/`FlightManager` in `ProxyAnimation` callbacks: value ticks
  rebuild the owner-local `Shuttle`, `ShuttleState::build` runs `on_tick`, and the
  `Send + Sync` status listener writes only a terminal-status flag. `TransitionRoute`
  no longer captures `TransitionInner` in the controller status listener: the listener
  queues `AnimationStatus` values, and owner-local `ModalScope::build` drains and
  applies route effects. `Focus::on_focus_change` now uses the same bridge pattern:
  the global focus listener records focus edges and schedules rebuild; the user
  handler runs from owner-local build, so callbacks may capture `NavigatorHandle`.
  `PageRoute`/`PopupRoute` page and transition builders no longer require
  `Send + Sync`; route builders are UI owner-plane. Test-only worker-thread deadlock
  harnesses that moved `NavigatorHandle` across threads were rewritten as direct
  owner-thread scenarios. Temporary `clippy::arc_with_non_send_sync` allowances were
  added to `flui-interaction`, `flui-view`, and `flui-widgets` with ADR-0027 comments:
  they document the current `Arc`-shaped owner-local handle graph and must not be
  interpreted as permission to restore `Send + Sync` to UI callbacks. Evidence:
  `cargo check -p flui-widgets --lib`, `cargo check -p flui-widgets --tests`,
  `cargo test -p flui-widgets transition_route --lib`, `cargo test -p flui-widgets
  hero_flight --lib`, `cargo test -p flui-widgets hero_seam --lib`,
  `cargo test -p flui-widgets focus --lib`, and `cargo clippy -p flui-widgets
  --all-targets -- -D warnings` pass.
- 2026-07-12 owner-plane hardening checkpoint: `FocusChangeHandler` moved from
  `Arc<dyn Fn(bool)>` to `Rc<dyn Fn(bool)>`, and `FocusState`'s live handler cell
  moved from `Arc<Mutex<Option<_>>>` to `Rc<RefCell<Option<_>>>`. The global
  `FocusManager` listener still captures only `Arc<Mutex<Vec<bool>>>` plus the
  rebuild handle: that queue is a data-only bridge from the `Send + Sync` manager
  seam into owner-local `build`, not executable UI ownership. Evidence:
  `cargo check -p flui-widgets --lib`, `cargo check -p flui-widgets --tests`,
  `cargo test -p flui-widgets focus --lib`, `cargo fmt --package flui-widgets
  -- --check`, and `cargo clippy -p flui-widgets --all-targets -- -D warnings`
  pass.
- 2026-07-12 route-builder hardening checkpoint: route and overlay builder
  closures moved from `Arc<dyn Fn>` to owner-local `Rc<dyn Fn>`:
  `RouteContentBuilder`, `RoutePageBuilder`, `RouteTransitionsBuilder`, and
  private `OverlayBuilder`. `RouteAnimation`, animation controllers,
  `ChangeNotifier`, and overlay/navigator handles remain `Arc` where they are
  data-plane or shared-handle seams. The `TransitionRoute` status bridge also
  gained a data-only `ChangeNotifier` wake so status changes without a value tick
  (for example `reverse()` from 1.0) still schedule owner-local `ModalScope`
  build to drain queued statuses; no executable UI callback moved into the
  `Send + Sync` listener. Evidence: `cargo check -p flui-widgets --lib`,
  `cargo check -p flui-widgets --tests`, `cargo test -p flui-widgets
  modal_route_tests --lib`, `cargo test -p flui-widgets page_route_tests --lib`,
  `cargo test -p flui-widgets navigator --lib`, `cargo test -p flui-widgets
  focus --lib`, `cargo fmt --package flui-widgets -- --check`, and `cargo
  clippy -p flui-widgets --all-targets -- -D warnings` pass.
- 2026-07-12 local-history/pop-scope hardening checkpoint: owner-plane route
  callbacks moved from `Arc<dyn Fn>` to `Rc<dyn Fn>` for `PopInvokedCallback`,
  `LocalHistoryEntry::on_remove`, and local-history's route-local
  `changed_internal_state` hook. Registry and entry handles remain `Arc` for
  now; this checkpoint only removes thread-safe ownership from executable
  callback payloads without changing the shared handle graph. Evidence:
  `cargo check -p flui-widgets --lib`, `cargo check -p flui-widgets --tests`,
  `cargo test -p flui-widgets local_history --lib`, `cargo test -p flui-widgets
  pop_scope --lib`, `cargo test -p flui-widgets navigator --lib`,
  `cargo fmt --package flui-widgets -- --check`, and `cargo clippy -p
  flui-widgets --all-targets -- -D warnings` pass.
- 2026-07-12 hero-hook hardening checkpoint: hero customization hooks moved to
  owner-local `Rc` callback payloads: `RectTweenFactory`, `ShuttleBuilder`, and
  `PlaceholderBuilder`. `Hero::create_rect_tween`,
  `HeroController::with_rect_tween`, and `Hero::flight_shuttle_builder` no
  longer require `Send + Sync` on the user factory/builder closure. The produced
  `Animatable<Rect>` remains `Send + Sync` for now because it is animation data,
  not executable UI ownership. Evidence: `cargo check -p flui-widgets --lib`,
  `cargo check -p flui-widgets --tests`, `cargo test -p flui-widgets
  hero_flight --lib`, `cargo test -p flui-widgets hero_controller --lib`,
  `cargo test -p flui-widgets hero_tests --lib`, `cargo fmt --package
  flui-widgets -- --check`, and `cargo clippy -p flui-widgets --all-targets --
  -D warnings` pass.
- 2026-07-12 lazy-builder/animated-builder hardening checkpoint: lazy sliver
  item builders moved from `Arc<dyn Fn(usize) -> Option<BoxedView>>` to
  owner-local `Rc<dyn Fn(...)>` across `flui-view::SliverList`,
  `SliverGridLazy`, their adaptor managers, and `flui-widgets`
  `SliverChildBuilderDelegate` / `ListView::builder` / `GridView::builder`.
  `AnimatedBuilder`'s rebuild closure also moved from `Arc<dyn Fn()>` to
  `Rc<dyn Fn()>`. Render/data seams were intentionally left alone:
  `ClipPath`'s clipper and `AnimatedSize::on_end` still enter render-object
  storage and need a separate render-plane bridge. `flui-view` integration
  tests/benchmarks that directly construct `ElementBuildContext` keep scoped
  ADR-0027 `arc_with_non_send_sync` allowances because that API still takes
  `Arc<RwLock<ElementTree/BuildOwner>>`; do not read those test allowances as
  permission to restore `Send + Sync` to owner-plane view/element state.
  Evidence: `cargo check -p flui-view --tests`, `cargo check -p flui-widgets
  --tests`, `cargo test -p flui-view sliver_adaptor --lib`, `cargo test -p
  flui-widgets lazy_list`, `cargo test -p flui-widgets lazy_grid`, `cargo test
  -p flui-widgets --test implicit_animations`, `cargo test -p flui-widgets
  --test fade_transition`, `cargo test -p flui-widgets --test
  rotation_transition`, `cargo test -p flui-widgets --test scale_transition`,
  `cargo fmt --package flui-view --package flui-widgets -- --check`, and
  `cargo clippy -p flui-view -p flui-widgets --all-targets -- -D warnings`
  pass.
- 2026-07-12 layout-builder hardening checkpoint: `LayoutWidgetBuilder` moved
  from `Arc<dyn Fn(&dyn BuildContext, BoxConstraints) -> BoxedView>` to
  owner-local `Rc<dyn Fn(...)>`. This keeps `LayoutBuilder`'s constraint-driven
  build callback in the UI owner plane; no render/data-plane storage is changed.
  Evidence: `cargo check -p flui-view --tests`, `cargo test -p flui-view
  layout_builder --lib`, `cargo test -p flui-widgets --test layout_builder`,
  `cargo fmt --package flui-view --package flui-widgets -- --check`, and
  `cargo clippy -p flui-view -p flui-widgets --all-targets -- -D warnings`
  pass.
- 2026-07-12 async-builder hardening checkpoint: `FutureFactory`,
  `StreamFactory`, `InitialDataFactory`, and `SnapshotBuilder` moved from
  `Arc<dyn Fn + Send + Sync>` to owner-local `Rc<dyn Fn>`. The async
  data-plane boundary remains intact: produced `BoxedResultFuture` and
  `BoxedResultStream` are still `Send`, and the shared snapshot slot remains
  `Arc<Mutex<_>>` because spawned tasks write it and the owner reads it.
  Evidence: `cargo check -p flui-view --tests`, `cargo check -p flui-widgets
  --tests`, `cargo test -p flui-view future_builder --lib`, `cargo test -p
  flui-view stream_builder --lib`, `cargo test -p flui-widgets --test
  future_builder`, `cargo test -p flui-widgets --test stream_builder`, and
  `cargo clippy -p flui-view -p flui-widgets --all-targets -- -D warnings`
  pass.
- 2026-07-12 refresh-indicator hardening checkpoint:
  `RefreshIndicator::on_refresh` moved from `Arc<dyn Fn() + Send + Sync>` to
  owner-local `Rc<dyn Fn()>`. The callback fires from `GestureDetector`'s
  owner-lane pan-end closure and does not enter render-object storage, the
  mouse/focus global trackers, or an async task. Evidence: `cargo check -p
  flui-widgets --tests` and `cargo test -p flui-widgets --test scroll
  refresh_indicator` pass.
- 2026-07-12 mouse-region owner-lane checkpoint: `MouseRegion` callbacks moved
  from `Arc<dyn Fn + Send + Sync>` to owner-local `Rc<dyn Fn>`, and
  `RenderMouseRegion` no longer stores executable enter/hover/exit callbacks.
  Render hit entries now carry a data-only `MouseTrackerAnnotation { region_id,
  target }`; the target resolves through `InteractionLane` to a strong
  owner-local callback cell. `MouseTracker` is owner-local (`Rc<RefCell<_>>`)
  with a thread-local global, derives active regions only from mouse
  annotations, preserves previous resolved annotations long enough to emit
  exit after target unregistration/removal, and continues later mouse callbacks
  before resuming the first panic. Hover remains ordinary pointer dispatch,
  matching Flutter's `RenderMouseRegion.handleEvent`; `MouseTracker` handles
  enter/exit/cursor updates. `BindingBase` and `RendererBinding` were updated
  to owner-runtime semantics so binding singletons are thread-local instead of
  process-global `Sync` objects. Evidence: `cargo test -p flui-interaction
  mouse_tracker --lib`, `cargo test -p flui-objects harness_mouse_region
  --test render_object_harness`, `cargo test -p flui-widgets --test
  mouse_region`, `cargo test -p flui-foundation binding --lib`, `cargo check
  -p flui-app --tests`, and `cargo clippy -p flui-foundation -p
  flui-interaction -p flui-rendering -p flui-objects -p flui-view -p
  flui-widgets -p flui-app --all-targets -- -D warnings` pass.
- 2026-07-12 Navigator ownership checkpoint (Task 6): `NavigatorHandle` is now
  structurally owner-affine (`!Send + !Sync`) while `NavigatorCommandTarget` and
  `NavigatorCommand` are the Send/Sync data-plane tokens. The command target
  stores only an opaque id plus owner `ThreadId`; the owner resolves it through
  a thread-local weak registry, so `NavigatorShared` and its route/view/observer
  storage never become cross-thread state. Cross-thread navigation enters
  through `UiCommandSender::send_navigation` and the closed `UiCommand::Navigation`
  vocabulary; the crate-internal closure `invoke` remains non-public and is not
  a navigation API. Route pushes stay owner-local because route builders carry
  owner-plane views. Typed error coverage exists for wrong-thread and dead-target
  application, and `UiRealm::drain_commands` drops dead navigation commands at
  the commit point. Evidence: `cargo test -p flui-widgets navigator_command
  --lib`, `cargo test -p flui-widgets
  navigator_handle_is_owner_affine_but_command_target_is_send_sync --lib`,
  `cargo test -p flui-widgets --lib`, `cargo test -p flui-app --lib`,
  `cargo fmt -p flui-widgets -p flui-app`, and `cargo clippy -p flui-widgets
  -p flui-app --all-targets -- -D warnings` pass.
- 2026-07-12 app-root authoring checkpoint (Task 7): the `flui-app` root-view
  bootstrap path no longer requires `Send + Sync` on app-authored roots.
  `run_app`, `run_app_with_config`, Android/web/desktop internal runners, and
  `AppBinding::attach_root_widget*` now require only owner-plane `View` /
  `StatelessView` + `Clone + 'static`. Platform callbacks still carry only the
  stamped realm dispatcher, renderer handles, and typed events after the root is
  attached; no root view crosses a thread boundary. Regression tests prove an
  `Rc<Cell<_>>` root can be named by the runner entrypoints and attached through
  `AppBinding` while data-plane wake/callback capabilities keep their `Send +
  Sync` bounds. Evidence: `cargo test -p flui-app
  attach_root_widget_accepts_owner_local_root_state --lib`, `cargo test -p
  flui-app runner_entrypoints_accept_owner_local_root_state --lib`, `cargo test
  -p flui-app --lib`, `cargo fmt -p flui-app`, and `cargo clippy -p flui-app
  --all-targets -- -D warnings` pass.
- 2026-07-12 key/action owner-local checkpoint (Task 7): the focus/key/action
  authoring surface no longer requires thread-safe UI callbacks.
  `FocusManager::global()` is now an owner-thread singleton backed by
  thread-local storage, and `FocusChangeCallback`, `KeyEventCallback`,
  `KeyEventHandler`, `CallbackAction`, erased action handlers,
  `CallbackShortcuts`, `Shortcuts`, and `EditableText` focus/key handlers use
  owner-local `Rc` callback payloads. `Intent` is also owner-local (`Any`
  only), and `Shortcuts` stores `Rc<dyn Intent>` so custom shortcut intents may
  carry `Rc<Cell<_>>` state. Data-plane seams remain unchanged: pointer/scroll
  routing, `RectProvider`, render objects, controller/listenable notifiers, and
  frame wake capabilities keep their existing thread-safe boundaries. Evidence:
  `cargo test -p flui-interaction focus --lib`, `cargo test -p flui-widgets
  shortcut_intents_accept_owner_local_rc_payloads --lib`, `cargo test -p
  flui-widgets shortcuts --lib`, `cargo test -p flui-widgets --lib`, `cargo
  fmt -p flui-interaction -p flui-widgets`, and `cargo clippy -p
  flui-interaction -p flui-widgets --all-targets -- -D warnings` pass.
- Tasks 3–5 must check the corresponding `.flutter/` sources before claiming parity.
  A green gate without behavioral evidence does not satisfy their acceptance slices.
- No task may introduce a generic UI-thread executor, queue the current pointer event,
  or move executable callbacks into `Send + Sync` render storage.
