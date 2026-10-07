# ADR-0086: Signal writes go through `EventCx` opened by a `WriterSource`

- **Status:** Proposed; the pilot passed (§9) and acceptance is the owner's call. Implemented:
  `EventCx`, `Writer`, `WriterSource`, lifecycle and render-context writer acquisition,
  callback inference helpers, typed `Signal` writes beside the transitional `&Reactive`
  target, and the owner-local catalog migration described below, including `Draggable`,
  `PageView::on_page_changed` (delivered on the local post-frame lane), the
  `Action::invoke`/`CallbackAction`/`CallbackShortcuts` family, and `DragTarget`'s and
  `Semantics`' callbacks through owner-local interaction-lane payloads (§3, amended
  2026-09-30). The rollback trigger was evaluated and not met (§9). Outstanding, each named
  again before the `!Send` flip of ADR-0091 §1:
  - `LocalHistoryEntry::on_remove`: a public surface and its navigation-flush write contract;
  - listener, animation-status and post-frame `cx` (§5), `TabController::add_listener`
    included;
  - `UiCommand::SignalWrite` opening its write through a `WriterSource` (the ADR-0074 §5.8
    amendment; the command still runs `FnMut(&Reactive)`);
  - `&Reactive` removal and both `reactive()` accessors (§8 step 3);
  - `StateCell::schedule` refused during `build` (§7).
- **Date:** 2026-09-25
- **Revised:** 2026-09-28 — the transitional `SignalWrite` callback is a retained `FnMut`
  envelope invoked at most once; success disposes captures after redraw demand is durable, while
  callback panic retains the opaque bundle because aggregate drop glue cannot be made
  recovery-safe. The planned `WriterSource` migration preserves that ownership contract.
- **Amends (on acceptance):** [ADR-0074](ADR-0074-realm-scoped-signals.md) — §5.1 (the signatures of `set`,
  `update` and `set_if_changed`), §5.2 (the run-time guard stays authoritative; `Writer` narrows
  it and does not replace it), §5.8 (`UiCommand::SignalWrite` opens its write through the
  UI runtime's `WriterSource`; pending, see Status); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §1 (one new
  `LifecycleContext` capability, `writer_source`)
- **Amends:** [ADR-0023](ADR-0023-actions-shortcuts-seam.md) §2 and §3 (`CallbackShortcuts`
  callbacks and `Action::invoke` receive the key event's `EventCx`; `Actions::maybe_invoke` is
  removed)
- **Related:** [ADR-0018](ADR-0018-async-builder-seam.md) (`RebuildHandle` stays a run-time
  capability), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §2 and §9 (UI callbacks are meant to
  be `!Send`), [ADR-0075](ADR-0075-derived-state-and-effects.md) (`WrittenDuringCompute`),
  [ADR-0076](ADR-0076-public-overlay-mutation-api.md) (the drag session is a gesture object and
  is not changed here), [ADR-0085](ADR-0085-reactive-core-placement-and-phase-subscribers.md)
  (graph placement, UI runtime routing, `ReadScope`)
- **Refs:** owner decision 7 in [`design/decisions.md`](../../design/decisions.md); the panel
  record in
  [`report-decisions.ru.md` §7](../research/2026-09-25-architecture-review/report-decisions.ru.md)
  and its probes in
  [`decisions/q7_callback_writer.md`](../research/2026-09-25-architecture-review/decisions/q7_callback_writer.md)

**Owner decision (2026-09-25).** The owner chose the typed form: `&mut EventCx<'_>` on framework
event callbacks through `WriterSource`, with the pilot and the rollback trigger of §8 as written.
This settles the shape question for the pilot; the record stays Proposed until the pilot ships
and the owner accepts it.

## Context

### Lineage

This is the third shape of signals in FLUI, and each step narrowed where a write can happen. The
removed `flui-reactivity` crate (added in `a57b41408`, deleted in `38620127f`, #486) had a
context-free `set(&self, value)` on a process-global `SIGNAL_RUNTIME` backed by a `DashMap`, with
everything `Send + Sync` and React-style hooks; it was deleted with no consumers, and its commit
message names the cross-UI runtime bleed a process-global runtime would cause. ADR-0074 replaced it
with UI runtime-scoped `Copy` handles and a run-time guard. Typed writes are the next step on the same
line: the UI runtime is already explicit for reads, and this record makes it explicit for writes.

### Writes are guarded only at run time

`Signal::set`, `update` and `set_if_changed` take `&Reactive`
(`crates/flui-view/src/reactive/mod.rs:774`, `:783`, `:793`). `Reactive` is `Clone`
(`mod.rs:160-164`) and every context hands it out: `BuildContext::reactive()`
(`crates/flui-view/src/context/build_context.rs:131-132`) is reachable from `build`. A write from
`build` is refused only when that path executes (`SignalError::WrittenDuringBuild`,
`mod.rs:578`); ADR-0078 §2 records the guard as the authoritative rule for this case. A test that
never reaches the write never sees the error.

At the original census, signals had no production users: no catalog crate, example or facade module created one
(`grep -rn "Signal<\|\.signal(" crates/flui-widgets/src packages/flui-material/src
packages/flui-cupertino/src examples` is empty; the only users are tests and the
`signals_rebuilds` bench). Changing the write signature now costs the catalog nothing it has
already written against signals.

### The callback surface is large and inconsistent

The catalog has 105 public `on_*` setters:

```text
grep -rhoE 'pub fn on_[a-z_]+' crates/flui-widgets/src packages/flui-material/src packages/flui-cupertino/src | wc -l   # 108
```

(The one hit that is not a setter is `on_drag_start` in `navigator/back_gesture.rs`, the
recognizer handler the back gesture calls.)

Most take `impl Fn(..) + 'static`; 23 take `impl Fn(..) + Send + Sync + 'static` (the five
`Draggable` setters, four `DragTarget` setters, `PageView::on_page_changed` and the fourteen
`Semantics` action setters). A `Signal<T>` is `!Send`, so none of those 23 can capture one today.
Five setters return a decision rather than reporting an event (the table below).

Beneath the setters sit framework callback types that are `Send + Sync` although ADR-0027 §2
lists UI callbacks as `!Send + !Sync`:

- `ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>`
  (`crates/flui-foundation/src/notifier.rs:46`) and `trait Listenable: Send + Sync` (`:78`);
- `StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>`
  (`crates/flui-animation/src/animation.rs:11`);
- `PostFrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>`
  (`crates/flui-scheduler/src/frame.rs:729`);
- platform hooks such as `set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>)`
  (`crates/flui-platform/src/traits/platform.rs:319`).

The gesture layer is already `!Send`: `TapCallback = Rc<dyn Fn(TapDetails)>`
(`crates/flui-interaction/src/recognizers/tap.rs:91`) and the drag aliases
(`crates/flui-interaction/src/recognizers/drag.rs:113-121`).

At the original census, one catalog callback was invoked from `build`: `AnimatedSize::build` delivered `on_end`
(`crates/flui-widgets/src/animated/animated_size.rs:201-209` in that revision).
The owner-local migration now delivers it after the frame. A callback called
from `build` cannot be handed a typed write capability, because `build` has none.

`StateCell` is bound from `init_state` through `StateCell::bind(&self, ctx: &dyn
LifecycleContext)` (`crates/flui-view/src/state_cell.rs:189`) and is unguarded while unbound by
design (`state_cell.rs:48-60`).

### What the panel measured

The probes in `decisions/q7_callback_writer.md` established that a write parameter on event
callbacks turns a write in `build` into a compile error (E0061, "provide the argument"); that a
`let`-bound closure passed where a higher-ranked `Fn(&mut EventCx<'_>)` is expected fails with
"implementation of `Fn` is not general enough" unless a helper fixes the signature; and that a
widget which holds a writer and calls a user callback synchronously inside its own `build` still
compiles, so the run-time guard cannot be removed under any shape
(`a_nested_sync_callback_during_build_still_needs_guard`).

## Decision

### 1. `EventCx` on framework event callbacks

Every framework-dispatched event callback in the table below receives `&mut EventCx<'_>`:

- borrowed, created for one dispatch, dropped when the callback returns;
- `Deref<Target = Writer>` and `DerefMut`;
- initially exactly a `Writer`. Anything else (spawning, focus, commands) is added only when a
  named setter needs it, by amending this record. It carries no tree position.

*Amended 2026-09-26:* the dispatching UI runtime's id, planned here, is deferred by that same rule:
`flui-view` has no UI runtime id to carry, and no setter needs one yet.

### 2. `Writer` is the write parameter

`Signal::set`, `update` and `set_if_changed` take `&mut Writer` in place of `&Reactive`. `peek`
is unchanged. `BuildContext::reactive()` and the public `BuildOwner::reactive()`
(`crates/flui-view/src/owner/build_owner.rs:972-974`) are removed in the same change, so `build`
has no path to a `Writer`. A `&mut Writer` cannot be stored beyond the dispatch that lent it.

`Writer` is a **narrowing of the run-time guard, not a replacement**. The guard stays
authoritative for the paths the type cannot see: a callback a widget invokes synchronously inside
its own `build`, and writes from a derived computation (`WrittenDuringCompute`, ADR-0075).

*Amended 2026-09-26:* the writes take `&W` where `W: WriteTarget`, a sealed trait implemented by
`EventCx<'_>`, `Writer` and, until step 3 of §8, `Reactive` —
`fn set<W: WriteTarget + ?Sized>(self, w: &W, value: T)`. A shared reference loses nothing: a
`Writer` still cannot escape its dispatch, because the `EventCx` lifetime bounds it, it is not
`Clone`, and it has no public constructor (the E0521 doctest on `WriterSource`). It keeps `cx`
reusable across several writes without explicit reborrows (`&mut EventCx` coerces to `&W`), and
it lets the unconverted `&Reactive` callers compile unchanged during the pilot.

### 3. `WriterSource` is the one way to open an `EventCx`

`LifecycleContext::writer_source()` returns a `WriterSource`: owned, `'static`, `!Send`, bound to
the UI runtime of the element that acquired it. `source.write(|cx| ..)` opens an `EventCx` for the
duration of the closure. It is the single mechanism used by

- catalog widgets, to wrap recognizer callbacks (§4);
- third-party widgets that expose their own `on_changed`-style callbacks;
- the framework's own non-dispatch write paths: task continuations and `UiCommand::SignalWrite`,
  which opens the write on the UI runtime that owns the slot (routing per ADR-0085 §1).

There is no second public "write handle" for foreign `Fn()` callbacks and no lint fencing the
catalog away from `WriterSource`. A handle for a same-thread `!Send` consumer that fits none of
the above is additive and waits for that consumer. Calling `write` while an element is building
is still refused by the guard.

*Amended 2026-09-26:* a `WriterSource` is bound to the reactive graph of one `BuildOwner` — one
presentation, not the whole UI runtime. A signal minted by another presentation's graph is refused
with `ForeignGraph` (logged, not applied), as a `&Reactive` write already was. A UI runtime-wide
source waits for the UI runtime core in `flui-runtime`.

`WriterSource` is a core UI runtime capability, not a platform capability, so it is a method on
`LifecycleContext` as ADR-0078 §1 prescribes; the open capability registry of
[ADR-0084](ADR-0084-open-capability-seam-and-plugins.md) does not apply to it.

*Amended 2026-09-27:* render views have no state lifecycle hook. Their
`RenderObjectContext::writer_source()` supplies the same presentation-bound
capability when they register owner-local pointer or hover handlers. Detached
contexts return `None`. The build context gains no writer; the render object
stores an interaction registration, not a reactive graph or an owner-local
closure. `tests/writer_source.rs` exercises the mounted render-view path.
`callback_with` and `callback_ref` fix inference for let-bound callbacks with
owned and borrowed payloads, respectively; these helpers do not change dispatch
or widen write authority.

*Amended 2026-09-30:* state that must stay owner-local but reaches its dispatcher through
`Send + Sync` render data — a drag target's slot in hit-test metadata, a semantics node's
action table behind `SemanticsConfiguration` — is registered in the UI runtime's interaction lane
as an untyped payload (`RenderObjectContext::register_local_payload`). The render data carries
only the lane's `Copy`, `Send + Sync` `LocalPayloadTarget` ticket, and the dispatcher resolves
it on the owner thread with `flui_interaction::resolve_local_payload`, inside the UI runtime's
entry. The payload holds the `WriterSource` its callbacks open their `EventCx` from; the ticket
holds neither a closure nor a graph. A semantics action is advertised through one
`Send + Sync` handler per node that holds the ticket, kept across rebuilds so the
configuration compares equal; invoked outside any UI runtime, it is dropped with a warning.
`DragTarget` and `Semantics` use it (the §6 rows); it adds no `static`.

### 4. The gesture arena does not change

`GestureArenaMember` and the recognizer callback aliases (`tap.rs:91`, `drag.rs:113-121`) keep
their signatures. A widget captures its `WriterSource` in the recognizer closure (legal: the
aliases are `Rc`) and calls `source.write(|cx| user_callback(cx, details))`. Consequences:
`flui-interaction` does not depend on the reactive graph, and custom recognizers do not break.

### 5. Listener, animation-status and post-frame callbacks

These receive `cx` only in the change that removes their `Send` bound. Until then a write from
them goes through a `SignalSender` and `UiCommand::SignalWrite`. Platform hooks write only
through `SignalSender`. This record does not schedule the removal of `Send` from the UI callback
surface — [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §1 does, before
the first crates.io publication, together with the setter signature change of §8 step 4; it
requires only that the two happen together, per callback family.

### 6. Query callbacks get no writer

A callback that returns a decision taken during routing or hit-testing gets no `cx`. The
classification, with the command above as its census:

| Crate / file | Setters | Class | `Send + Sync` today | Dispatch site |
|---|---|---|---|---|
| widgets `animated/animated_size.rs` | `on_end` | event | no | **`build`** (`animated_size.rs:201-209`); moves to a status listener or post-frame first |
| widgets `interaction/dismissible.rs` | `on_resize`, `on_dismissed`, `on_update` | event | no | not yet audited |
| widgets `interaction/draggable.rs` | `on_drag_started`, `on_drag_update`, `on_draggable_canceled`, `on_drag_end`, `on_drag_completed` | event | no (was yes) | recognizer handle, through the draggable's `WriterSource`; the unmount cancel runs from `dispose` in `finalize_tree`, outside any build |
| widgets `interaction/drag_target.rs` | `on_accept`, `on_leave`, `on_move` | event | no (was yes) | the drag session, synchronously inside pointer dispatch, through the target's `WriterSource`; `on_accept` before the draggable's `on_drag_end` |
| widgets `interaction/drag_target.rs` | `on_will_accept` (`-> bool`) | **query** | no (was yes) | — |
| widgets `interaction/focus.rs` | `on_focus_change`, `on_key_event` (`-> KeyEventResult`) | event | no | not yet audited |
| widgets `interaction/gesture_detector.rs` | 13: `on_tap`, `on_secondary_tap`, `on_long_press`, `on_double_tap`, `on_double_tap_down`, `on_pan_{start,update,end}`, `on_horizontal_drag_{down,start,update,end,cancel}` | event | no | recognizer (§4); `on_tap` and `on_long_press` also from an assistive-technology request, after the frame through the local post-frame lane, but **inside `build`** when the context has no such lane (`drain_semantics_requests`, `gesture_detector.rs:540-543`) |
| widgets `interaction/raw_button.rs` | `on_press` | event | no | `GestureDetector::on_tap` through the button's `WriterSource` (takes `cx` already) |
| widgets `interaction/interactive_viewer.rs` | `on_interaction_{start,update,end}` | event | no | not yet audited |
| widgets `interaction/listener.rs` | `on_pointer_{down,up,move,hover,cancel,signal}`, `on_pointer_pan_zoom_update` | event | no | pointer dispatch |
| widgets `interaction/listener.rs` | `on_scroll_claim`, `on_pointer_pan_zoom_claim` (`-> EventPropagation`) | **query** | no | — |
| widgets `interaction/mouse_region.rs` | `on_enter`, `on_hover`, `on_exit` | event | no | not yet audited |
| widgets `navigator/navigator.rs` | `on_generate_route`, `on_unknown_route` (`-> Option<GeneratedRoute>`) | **query** | no | — |
| widgets `navigator/pop_scope.rs` | `on_pop_invoked` | event | no | not yet audited |
| widgets `scroll/page_view.rs` | `on_page_changed` | event | no (was yes) | the controller listener records the page; delivered on the local post-frame lane |
| widgets `scroll/refresh_indicator.rs` | `on_refresh` | event | no | not yet audited |
| widgets `semantics/mod.rs` | 14: `on_tap`, `on_long_press`, `on_scroll_{left,right,up,down}`, `on_increase`, `on_decrease`, `on_show_on_screen`, `on_focus`, `on_blur`, `on_set_text`, `on_scroll_to_offset`, `on_action` | event | no (was yes) | the UI runtime's semantics-action drain, synchronously, through the node's `WriterSource` |
| widgets `text/editable_text.rs`, `text/text_field.rs` | `on_submitted` ×2 | event | no | not yet audited |
| widgets `text/editable_text.rs`, `text/text_field.rs` | `on_changed` ×2 | event | no | the field's key handler, IME commit and clipboard actions, after a user edit |
| widgets `navigator/local_history.rs` | `on_remove` | event | no | not yet audited |
| widgets `form/mod.rs` | `Form::on_changed` | event | no | a handle method the caller invokes: `FormFieldHandle::did_change`/`reset`, `FormHandle::reset` |
| widgets `form/form_field.rs`, `form/raw_text_form_field.rs` | `on_saved` ×2, `on_reset` ×2, `on_submitted` | event | no | `on_saved` from `FormHandle::save`, `on_reset` from `FormHandle::reset`/`FormFieldHandle::reset`; `on_submitted` as `EditableText`'s |
| material `text_field.rs`, `text_form_field.rs` | `on_changed`, `on_saved`, `on_reset`, `on_submitted` | event | no | as the widgets rows above |
| material (19 files) | 25: `on_pressed` ×7, `on_tap` ×4, `on_changed` ×3, `on_deleted` ×2, `on_selected`, `on_select_changed`, `on_select_all`, `on_open_changed`, `on_destination_selected`, `on_drawer_changed`, `on_end_drawer_changed`, `on_closed`, `on_submitted` | event | no | not yet audited |
| cupertino (2 files) | `on_tap`, `on_pressed`, `on_long_press` | event | no | not yet audited |

`CallbackShortcuts::binding` and `CallbackAction::new` are not `on_*` setters, so the census
does not count them, but they are event callbacks run from `Focus::on_key_event`, and they and
`Action::invoke` receive the key event's `cx`. `Action::is_enabled` and
`Action::to_key_event_result` are queries and get none.

Totals: 101 event, 5 query, 106 in all. (The grep prints 108: besides `on_drag_start`, it
counts `TextFormFieldInput::on_changed` in `form/text_form_field_core.rs`, a method that returns
the field's handler rather than a setter.) `on_key_event` is the one event callback that returns a
value: it runs at key dispatch after routing, and keyboard shortcuts are a primary write path, so
it receives `cx` and keeps its `KeyEventResult`. The "dispatch site" column must be complete —
every row audited for a call from `build`, a listener or a post-frame callback — before any
setter signature changes.

The form's callbacks are the one family dispatched from methods the application calls rather
than from the framework: `on_saved` runs inside `FormHandle::save`, `on_reset` inside
`FormHandle::reset` and `FormFieldHandle::reset`, and `Form::on_changed` inside those and
`FormFieldHandle::did_change`. In step 4 of §8 those handle methods take the caller's
`&mut EventCx<'_>` and pass it to the callbacks they run — `FormHandle::save(&self, cx: &mut
EventCx<'_>)` — and a text form field's input forwards the `cx` its own `on_changed` receives into
`did_change`; `flui migrate` rewrites the handle calls with the setters. The three `validator`
setters (`FormField`, `RawTextFormField`, the Material `TextFormField`; not `on_*`, so outside
the count) return a decision and are queries: they get no writer.

### Owner-local catalog migration record

The table in §6 is the original census, not a claim that every setter has been
migrated. Reproduce the current setter inventory with
`rg -n 'pub fn on_' crates/flui-widgets/src packages/flui-material/src packages/flui-cupertino/src`;
the signatures and production dispatch sites decide the event/query classification.

| Migrated family | Dispatch and ownership |
|---|---|
| GestureDetector, Listener, MouseRegion | Recognizer/pointer dispatch; the mounted presentation supplies the writer. Recognizer contracts are unchanged. |
| Focus and editable text | Focus/key, IME and clipboard dispatch; lifecycle autofocus is outside the signal build guard. The focus regression tests include initial mount, reconfiguration and node replacement. |
| Forms | `save`, `reset` and `did_change` validate owner identity and build phase before mutation, then forward the caller's context; validators remain queries. |
| Material and Cupertino controls | Gesture contexts pass through composition; owner-local keyboard bridges acquire a source in lifecycle. Drawer callbacks use their mounted controller's source. |
| ScaffoldMessenger completion | Direct removal stays synchronous; build-observed completion uses the existing local post-frame lane. Disposal cancels pending completion. No inline fallback during build. |
| AnimatedSize and Dismissible | Build/layout-observed effects use local post-frame delivery. Dismissible's direct input-time dismissal remains synchronous. |
| InteractiveViewer, RefreshIndicator, PopScope | Gesture/scroll/pop dispatch; query signatures stay unchanged, and owner-local bridges supply the writer where the lower-level protocol has no context. |
| Draggable | The owner-local drag session opens each callback's write through the draggable's `WriterSource`. The config is `Rc<RefCell<_>>`, never borrowed across user code. An unmount cancels from `dispose` in `finalize_tree`, where the writes land; the feedback layer is removed before that cancel runs user code. |
| PageView | The `Send + Sync` controller listener only records the page and schedules a rebuild; `build` hands each recorded page to the local post-frame lane, one entry per page, and delivery reads the current callback. No lane: dropped with a warning, never run inside `build`. |
| Actions and CallbackShortcuts | `Focus::on_key_event` hands its `cx` to `CallbackShortcuts` callbacks and to `Action::invoke`. `Actions::maybe_invoke` is removed: its only possible caller was `build`. InkWell's keyboard activation drops its writer bridge. |
| DragTarget | The slot (entered drags, callbacks, rebuild handle, writer) is an owner-local `Rc` registered in the interaction lane; hit-test metadata carries only its `LocalPayloadTarget`, which the drag session resolves inside pointer dispatch. `on_accept` runs inside `finish_drag`, before the draggable's `on_drag_end`. `on_will_accept` stays a query and loses `Send + Sync`. |
| Semantics | Each node's action table and writer are one lane payload; the configuration advertises every action through one `Send + Sync` handler holding the ticket, reused across rebuilds so the configuration compares equal. The handler runs in the UI runtime's semantics-action drain; invoked outside a UI runtime, the action is dropped with a warning. A detached mount advertises none. `GestureDetector` keeps its post-frame bridge onto `on_tap`/`on_long_press`. |

Callback ownership moved before any `Send` bound was removed: both render objects stay
`Send + Sync` and carry only tickets. What remains outside this record: `LocalHistoryEntry` is
an unmounted navigation primitive whose removal also runs through navigator flush, so its
originating-context contract is settled with that dispatch path; the `TabController` and
`CupertinoTabController` listeners notify from a method the application calls, so their `cx`
arrives with the listener family (§5) as a caller-supplied context, as the form handles do;
`UiCommand::SignalWrite` still opens its write on `&Reactive` (§3, ADR-0074 §5.8); the
`&Reactive` target and both `reactive()` accessors go in §8 step 3; and `StateCell::schedule`
is refused during `build` per §7. These boundaries follow §5 rather than weakening types or
adding an ambient graph. The inventory of what still requires `Send` on the callback path —
the families ADR-0091 §1 flips, the edges that stay `Send` by design, and the `Send + Sync`
values that are data rather than callbacks — is kept with the `!Send` flip's row of
[the architecture migration plan](../plans/2026-09-25-architecture-migration-plan.md).

`FloatingActionButton::new(child).on_pressed(callback)` replaces the optional
generic constructor callback: nested `Some(closure)` prevented higher-ranked
inference. The ordinary setter accepts an unannotated inline signal closure,
pinned by its mounted integration test. No new dependency or executor is needed.

**Ownership preflight.** Form handles return `Result<_, EventContextError>`:
detached handles, foreign presentation contexts and calls during build are
refused before mutation. A form preflights its field snapshot before visiting
the first field. User callbacks remain arbitrary effects, so this is not
transactional rollback if a callback panics or detaches later fields. Silently
opening the target's writer for a foreign caller is rejected: cross-presentation
commands need an explicit target and delivery contract.

`EventCx` and its borrowed `Writer` implement the read-only `ReadGraph` face:
`signal.peek(cx, ...)` sees current state without subscribing or invalidating
an element. The context is a graph capability, not a physical event identifier,
propagation state or async lifetime. Independently opened writer bridges must
not be treated as one transaction merely because they share a graph.

The [design challenge](../research/2026-09-27-event-context-design-review.md)
records failure scenarios, rejected alternatives and remaining ownership and
overload boundaries. The post-frame bridge does not settle the shared animation
listener topology or promise bounded event latency.

### 7. `StateCell` and `RebuildHandle` stay run-time capabilities

Neither takes `&mut Writer`. `StateCell` is already a capability bound in `init_state`
(`state_cell.rs:189`) and works unbound; `RebuildHandle` stays as ADR-0018 defines it. The guard
is extended so that `StateCell::schedule` during `build` is refused like a signal write. The
compile-time claim of this record is limited to writes into `Signal<T>`.

### 8. Order and rollback

The changes land one at a time, each with `cargo xtask check-changed` green:

1. ADR-0085 §1 (UI runtime routing of `UiCommand::SignalWrite`), with its failing multi-presentation
   test. It does not depend on anything else here.
2. `EventCx`, `Writer`, `WriterSource`, and a `callback(|cx| ..)` helper that fixes the
   higher-ranked signature, named in the widget-author documentation.
3. The `Signal` write signatures, and removal of both `reactive()` accessors.
4. The setter signatures, migrated crate by crate by `flui migrate`, starting with a pilot. The
   pilot surface is one new catalog widget, `flui_widgets::RawButton` (its `on_press` takes
   `Fn(&mut EventCx<'_>)` and wraps an unchanged `GestureDetector::on_tap` through a
   `WriterSource`), and the application code that uses it: the `counter` and `todo` examples
   (`examples/counter.rs`, `examples/todo.rs`) and the `flui create` counter template
   (`crates/flui-cli/src/templates/counter.rs`). Those examples previously used only Material
   setters and `StateCell`/`StateHandle` (untyped by §7), so they move to the widgets catalog
   and to `Signal<T>` state. `flui-cupertino`'s three setters move to the crate-by-crate
   migration with `GestureDetector`'s thirteen: converting either inside the pilot would touch
   Material and Cupertino call sites and make the pilot costly to revert. Steps 3 and 4 land
   before the first crates.io publication (ADR-0091 §1).

`Writer` and `WriterSource` live in `flui-view`, beside the graph (ADR-0085 §6).

**Rollback to guard-only.** If the pilot needs explicit closure type
annotations at call sites that `callback(..)` does not cover, or the converted call sites are
materially longer than the probe's, the design switches before 1.0 to the guard-only shape: the
`Signal` handle resolves its UI runtime itself, writes are allowed anywhere outside `build`, and the
run-time guard is the only enforcement. The pilot's diff and the decision are recorded by
amending this record.

**Pilot thresholds.** Fixed before any pilot code was written:

- *Converted sites* are five: the counter example's Increment, the todo example's Toggle,
  Delete and Add, and the template's Increment. A site's guard-only counterfactual is the same
  code with `|cx|` replaced by `||` and each `cx, ` argument removed.
- *Excess* at a site is anything else that differs from its counterfactual: `let _`, `.ok()`,
  clones, type annotations, extra lines.
- The trigger **fires** if any converted site needs a closure parameter annotation that
  `callback` does not remove, or if any converted site carries excess beyond passing `cx` into a
  helper's parameter (the probe's pattern).
- Reported but outside the trigger: the excess at the one transitional site (the todo field's
  Enter handler, which stays on an unconverted `Fn(&str)` setter and opens its write through a
  held `WriterSource`); the diff from moving state to `Signal` (the cost of ADR-0074 and
  ADR-0085, not of this record); collateral edits to non-pilot code, expected to be none.
- Measured with `git diff --numstat` against the base in five groups (application code, the
  catalog widget, the core crates, tests, docs); `git grep -nE '\|[a-z_]+: *&mut EventCx' --
  examples crates/flui-cli/src/templates` (annotations, expected empty); `git grep -c
  'callback(' -- examples` (helper uses); and, for each compile-fail snapshot, the error code,
  its first line, whether the primary label points at the offending token, and whether a note
  names the fix.

### 9. Pilot record

Measured on 2026-09-26 against base `e91ec071d`, on rustc 1.98.1, with the definitions of §8
fixed before the code.

**Converted sites.** All five match their guard-only counterfactual except for `cx`:

| Site | Code | Excess |
|---|---|---|
| counter, Increment | `.on_press(move \|cx\| count.update(cx, \|n\| *n += 1))` | none |
| todo, Toggle | `.on_press(move \|cx\| { items.update(cx, \|list\| { .. }) })` | none |
| todo, Delete | `.on_press(move \|cx\| { items.update(cx, \|list\| list.retain(..)) })` | none |
| todo, Add | `.on_press(move \|cx\| { add_item(cx, items, &button_field, &button_field.text()) })` | `cx` passed to the helper's parameter (the probe's pattern) |
| template, Increment | as the counter | none |

The press callbacks return the write's `Result` (`EventOutcome`), so no site needs `let _` or
`.ok()`; `Signal` is `Copy`, so none needs a clone. `git grep -nE '\|[a-z_]+: *&mut EventCx' --
examples crates/flui-cli/src/templates` finds nothing (no annotation), and `git grep -c
'callback(' -- examples` finds nothing (no site needed the helper).

**Reported outside the trigger.**

- *The transitional site*, the todo field's Enter handler on the unconverted
  `RawTextField::on_submitted(Fn(&str))`: 11 lines of excess — a `writer: Option<WriterSource>`
  field, its `None` in `create_state`, `self.writer = Some(ctx.writer_source())` in
  `init_state`, a four-line `let writer = ..expect(..)` in `build`, the call growing from one
  line to five (`writer.write(|cx| add_item(cx, ..)).report();` in a block, as rustfmt lays it
  out), and `EventOutcome` added to an existing `use`. `.report()` logs a refused write, as
  `RawButton` does with a press's `Result`. It disappears when the setter
  itself takes `cx`.
- *Moving state to `Signal`*: `#[derive(Default)]` on the state and `Signal::default()` in place
  of `StateCell::new(0)`/`StateHandle::new(..)`; net one line shorter in the counter.
- *Collateral outside the pilot*: none in code. One layout defect surfaced and was fixed in the
  pilot file itself: the todo `ListView` sat in a bare `Column`, got unbounded height, and its
  rows were never reachable (not in the semantics tree, not hit); it is now `Expanded`.

**Diff size** (`git diff --numstat e91ec071d...` at the pilot's last code commit, added/removed):
application code (both examples and the template) +84/−84; the catalog widget
(`raw_button.rs`) +128, with +17/−13 of exports; core (`flui-view/src`, `flui-foundation/src`)
+564/−15, of which about 180 are unit tests and most of the rest doc comments; tests +584; docs
+204/−105; `Cargo.toml` +4/−10.

**Compiler errors** (the `tests/ui` snapshots in `flui-view`):

| Case | Code and first line | Primary label on the offending token | Fix named |
|---|---|---|---|
| `s.set(ctx, 1)` with `ctx: &dyn BuildContext` | E0277 "`dyn BuildContext` cannot write a signal" | yes, `ctx`: "not a write context" | yes, two notes (take the callback's `cx`; acquire a `WriterSource` in `init_state`). The implementor list also shows `Reactive`, which points at `ctx.reactive()` until step 3 of §8 |
| `s.set(1)` | E0061 "this method takes 2 arguments but 1 argument was supplied" | yes, `set`: "argument #1 of type `&_` is missing" | generically: `count.set(/* w */, 1)` |
| `callback(move \|\| ..)` | E0593 "closure is expected to take 1 argument, but it takes 0 arguments" | on the call; the closure is labelled "takes 0 arguments" | yes: `move \|_\|` |
| a `let`-bound `move \|cx\| count.set(cx, 1)` passed to `write` | E0631 "type mismatch in closure arguments" | yes, the closure argument | a working but clumsy wrapper; `callback` is not named. The mitigation is `callback`'s documentation |
| `&mut **cx` stored outside the closure | E0521 "borrowed data escapes outside of closure" | yes: "`cx` escapes the closure body here" | no (the pattern has no fix) |

The probe's "implementation of `Fn` is not general enough" did not reproduce: on this toolchain a
`let`-bound closure annotated `|cx: &mut EventCx|` compiles, and only an unannotated one fails
(E0631 above). `callback` still removes the annotation.

**Inference assumptions.** Both hold: `&mut EventCx` coerces to a generic `&W` (the `callback`
doctest makes two writes through one `cx`), and a closure whose return type `R` is generic is
inferred higher-ranked where `Fn(&mut EventCx<'_>) -> R` is expected (`RawButton::on_press`,
`callback`, `WriterSource::write`).

**Verdict.** The trigger did not fire: no converted site needs an annotation, and no converted
site carries excess beyond passing `cx` to a helper. The typed shape stands, and the record is
ready for the owner to accept.

## Alternatives considered

- **Ambient write scope (a thread-local "current writer").** Rejected: it needs the same wiring
  as `EventCx`, fails at run time (`NoScope`) instead of at compile time, and adds a thread-local
  against ADR-0097.
- **Guard only; the handle finds its UI runtime** (the rollback shape). Not chosen now: cheapest to
  build, but after 1.0 it cannot be tightened without breaking every callback. It stays the
  recorded fallback.
- **`EventCx` plus a separate `WriteHandle` for foreign callbacks, fenced out of the catalog by
  `disallowed_methods`.** Rejected: before the `Send` bounds go, the handle has almost no
  compilable use, and the fence would forbid exactly the recognizer wrapping §4 needs.
- **Put `Writer` into the gesture arena** (`GestureArenaMember` and the recognizer aliases take
  `&mut Writer`). Rejected: it breaks custom recognizers and makes `flui-interaction` depend on
  the reactive graph, which lives in `flui-view` above it.
- **Accept both `Fn()` and `Fn(&mut EventCx)` during a transition.** Rejected: new-style closures
  then need explicit type annotations (probe d1), which is worse than one shape.
- **Give `StateCell` a `&mut Writer` parameter.** Rejected: `StateCell` is a capability already,
  it deliberately works unbound, and typing it would break every stateful widget for no new
  guarantee.

## Consequences

- A signal write in `build` becomes a compile error at the call site. Writes from nested
  synchronous callbacks during `build` and from computations remain run-time errors.
- ADR-0074 §5.1's signatures, §5.2's enforcement statement and §5.8's command path are replaced
  as stated in the header; its semantics stand. ADR-0078 §1 gains `writer_source` in its method
  list.
- **Breaks.** Every event setter in §6 changes its closure signature (100 setters; the panel
  counted about 432 call sites, an upper-bound grep). `Signal::set/update/set_if_changed`
  change their first parameter. `BuildContext::reactive()` and `BuildOwner::reactive()` are
  removed. Custom recognizers and query callbacks do not change.
- **Migration.** `flui migrate` rewrites setter call sites crate by crate; a hand-written closure
  that ignores the context becomes `callback(|_cx| ..)` or `|_cx| ..`. A widget calling a user
  callback from `build` must move that call to a listener or post-frame callback first.
- `WriterSource` interacts with the `!Send` flip: until listener and post-frame callbacks lose
  `Send`, a `WriterSource` cannot be captured in them. ADR-0091 §1 schedules that flip before the
  first crates.io publication; it delivers `cx` to each callback family in the same change.

## Verification

Marked **(exists)** where the item is in the repository.

- `compile_fail` checks: `signal.set(..)` through the `build` context does not compile
  **(partly exists**: `tests/ui/signal_write_through_build_context.rs` refuses
  `s.set(ctx, 1)`; `s.set(&ctx.reactive(), 1)` still compiles until step 3 of §8 removes the
  `&Reactive` target and the accessor**)**; a
  `&mut Writer` does not escape into a `'static` closure **(exists**: the E0521 doctest on
  `WriterSource`, `tests/ui/writer_escapes_the_dispatch.rs`**)**; `build` has no method
  returning a `Writer` **(exists**: the E0599 and E0624 doctests on
  `LifecycleContext::writer_source`**)**; `WriterSource`, `Writer` and `EventCx` are `!Send`
  **(exists**: `static_assertions` in `reactive/writer.rs`**)**.
- A test that a widget invoking a user callback synchronously inside its own `build`, through a
  captured `WriterSource`, is refused by the guard **(exists**:
  `flui-view/tests/writer_source.rs::a_callback_run_inside_its_widgets_build_is_refused_by_the_guard`**)**.
- A test that `AnimatedSize` no longer calls `on_end` from `build` **(exists**:
  `crates/flui-widgets/tests/animated_size.rs::animated_size_completion_writes_a_signal_after_build`**)**.
- A test that `StateCell::schedule` during `build` is refused.
- ADR-0085's multi-presentation routing test, now written through `WriterSource`.
- The completed §6 table, reproduced by its grep command, in the change that alters setter
  signatures; `flui migrate` applied to `examples/` with `cargo xtask check-changed` green.
