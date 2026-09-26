# ADR-0086: Signal writes go through `EventCx` opened by a `WriterSource`

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0074](ADR-0074-realm-scoped-signals.md) — §5.1 (the signatures of `set`,
  `update` and `set_if_changed`), §5.2 (the run-time guard stays authoritative; `Writer` narrows
  it and does not replace it), §5.8 (`UiCommand::SignalWrite` opens its write through the
  realm's `WriterSource`); [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §1 (one new
  `LifecycleContext` capability, `writer_source`)
- **Related:** [ADR-0018](ADR-0018-async-builder-seam.md) (`RebuildHandle` stays a run-time
  capability), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §2 and §9 (UI callbacks are meant to
  be `!Send`), [ADR-0075](ADR-0075-derived-state-and-effects.md) (`WrittenDuringCompute`),
  [ADR-0076](ADR-0076-public-overlay-mutation-api.md) (the drag session is a gesture object and
  is not changed here), [ADR-0085](ADR-0085-reactive-core-placement-and-phase-subscribers.md)
  (graph placement, realm routing, `ReadScope`)
- **Refs:** owner decision 7 in [`design/decisions.md`](../../design/decisions.md); the panel
  record in
  [`report-decisions.ru.md` §7](../research/2026-09-25-architecture-review/report-decisions.ru.md)
  and its probes in
  [`decisions/q7_callback_writer.md`](../research/2026-09-25-architecture-review/decisions/q7_callback_writer.md)

**Owner decision (2026-09-25).** The owner chose the typed form: `&mut EventCx<'_>` on framework
event callbacks through `WriterSource`, with the pilot and the rollback trigger of §8 as written.
This settles the shape question for the pilot; the record stays Proposed until the pilot ships.

## Context

### Lineage

This is the third shape of signals in FLUI, and each step narrowed where a write can happen. The
removed `flui-reactivity` crate (added in `a57b41408`, deleted in `38620127f`, #486) had a
context-free `set(&self, value)` on a process-global `SIGNAL_RUNTIME` backed by a `DashMap`, with
everything `Send + Sync` and React-style hooks; it was deleted with no consumers, and its commit
message names the cross-realm bleed a process-global runtime would cause. ADR-0074 replaced it
with realm-scoped `Copy` handles and a run-time guard. Typed writes are the next step on the same
line: the realm is already explicit for reads, and this record makes it explicit for writes.

### Writes are guarded only at run time

`Signal::set`, `update` and `set_if_changed` take `&Reactive`
(`crates/flui-view/src/reactive/mod.rs:774`, `:783`, `:793`). `Reactive` is `Clone`
(`mod.rs:160-164`) and every context hands it out: `BuildContext::reactive()`
(`crates/flui-view/src/context/build_context.rs:131-132`) is reachable from `build`. A write from
`build` is refused only when that path executes (`SignalError::WrittenDuringBuild`,
`mod.rs:578`); ADR-0078 §2 records the guard as the authoritative rule for this case. A test that
never reaches the write never sees the error.

Signals have no production users yet: no catalog crate, example or facade module creates one
(`grep -rn "Signal<\|\.signal(" crates/flui-widgets/src crates/flui-material/src
crates/flui-cupertino/src examples` is empty; the only users are tests and the
`signals_rebuilds` bench). Changing the write signature now costs the catalog nothing it has
already written against signals.

### The callback surface is large and inconsistent

The catalog has 105 public `on_*` setters:

```text
grep -rhoE 'pub fn on_[a-z_]+' crates/flui-widgets/src crates/flui-material/src crates/flui-cupertino/src | wc -l   # 106
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

One catalog callback is invoked from `build`: `AnimatedSize::build` delivers `on_end`
(`crates/flui-widgets/src/animated/animated_size.rs:201-209`). A callback called from `build`
cannot be handed a typed write capability, because `build` has none.

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
- initially exactly a `Writer` plus the dispatching realm's id. Anything else (spawning, focus,
  commands) is added only when a named setter needs it, by amending this record. It carries no
  tree position.

### 2. `Writer` is the write parameter

`Signal::set`, `update` and `set_if_changed` take `&mut Writer` in place of `&Reactive`. `peek`
is unchanged. `BuildContext::reactive()` and the public `BuildOwner::reactive()`
(`crates/flui-view/src/owner/build_owner.rs:972-974`) are removed in the same change, so `build`
has no path to a `Writer`. A `&mut Writer` cannot be stored beyond the dispatch that lent it.

`Writer` is a **narrowing of the run-time guard, not a replacement**. The guard stays
authoritative for the paths the type cannot see: a callback a widget invokes synchronously inside
its own `build`, and writes from a derived computation (`WrittenDuringCompute`, ADR-0075).

### 3. `WriterSource` is the one way to open an `EventCx`

`LifecycleContext::writer_source()` returns a `WriterSource`: owned, `'static`, `!Send`, bound to
the realm of the element that acquired it. `source.write(|cx| ..)` opens an `EventCx` for the
duration of the closure. It is the single mechanism used by

- catalog widgets, to wrap recognizer callbacks (§4);
- third-party widgets that expose their own `on_changed`-style callbacks;
- the framework's own non-dispatch write paths: task continuations and `UiCommand::SignalWrite`,
  which opens the write on the realm that owns the slot (routing per ADR-0085 §1).

There is no second public "write handle" for foreign `Fn()` callbacks and no lint fencing the
catalog away from `WriterSource`. A handle for a same-thread `!Send` consumer that fits none of
the above is additive and waits for that consumer. Calling `write` while an element is building
is still refused by the guard.

`WriterSource` is a core realm capability, not a platform capability, so it is a method on
`LifecycleContext` as ADR-0078 §1 prescribes; the open capability registry of
[ADR-0084](ADR-0084-open-capability-seam-and-plugins.md) does not apply to it.

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
| widgets `interaction/draggable.rs` | `on_drag_started`, `on_drag_update`, `on_draggable_canceled`, `on_drag_end`, `on_drag_completed` | event | yes | not yet audited |
| widgets `interaction/drag_target.rs` | `on_accept`, `on_leave`, `on_move` | event | yes | not yet audited |
| widgets `interaction/drag_target.rs` | `on_will_accept` (`-> bool`) | **query** | yes | — |
| widgets `interaction/focus.rs` | `on_focus_change`, `on_key_event` (`-> KeyEventResult`) | event | no | not yet audited |
| widgets `interaction/gesture_detector.rs` | 13: `on_tap`, `on_secondary_tap`, `on_long_press`, `on_double_tap`, `on_double_tap_down`, `on_pan_{start,update,end}`, `on_horizontal_drag_{down,start,update,end,cancel}` | event | no | recognizer (§4) |
| widgets `interaction/interactive_viewer.rs` | `on_interaction_{start,update,end}` | event | no | not yet audited |
| widgets `interaction/listener.rs` | `on_pointer_{down,up,move,hover,cancel,signal}`, `on_pointer_pan_zoom_update` | event | no | pointer dispatch |
| widgets `interaction/listener.rs` | `on_scroll_claim`, `on_pointer_pan_zoom_claim` (`-> EventPropagation`) | **query** | no | — |
| widgets `interaction/mouse_region.rs` | `on_enter`, `on_hover`, `on_exit` | event | no | not yet audited |
| widgets `navigator/navigator.rs` | `on_generate_route`, `on_unknown_route` (`-> Option<GeneratedRoute>`) | **query** | no | — |
| widgets `navigator/pop_scope.rs` | `on_pop_invoked` | event | no | not yet audited |
| widgets `scroll/page_view.rs` | `on_page_changed` | event | yes | not yet audited |
| widgets `scroll/refresh_indicator.rs` | `on_refresh` | event | no | not yet audited |
| widgets `semantics/mod.rs` | 14: `on_tap`, `on_long_press`, `on_scroll_{left,right,up,down}`, `on_increase`, `on_decrease`, `on_show_on_screen`, `on_focus`, `on_blur`, `on_set_text`, `on_scroll_to_offset`, `on_action` | event | yes | assistive-technology action dispatch |
| widgets `text/editable_text.rs`, `text/text_field.rs` | `on_submitted` ×2 | event | no | not yet audited |
| widgets `text/editable_text.rs`, `text/text_field.rs` | `on_changed` ×2 | event | no | the field's key handler, IME commit and clipboard actions, after a user edit |
| widgets `navigator/local_history.rs` | `on_remove` | event | no | not yet audited |
| widgets `form/mod.rs` | `Form::on_changed` | event | no | a handle method the caller invokes: `FormFieldHandle::did_change`/`reset`, `FormHandle::reset` |
| widgets `form/form_field.rs`, `form/raw_text_form_field.rs` | `on_saved` ×2, `on_reset` ×2, `on_submitted` | event | no | `on_saved` from `FormHandle::save`, `on_reset` from `FormHandle::reset`/`FormFieldHandle::reset`; `on_submitted` as `EditableText`'s |
| material `text_field.rs`, `text_form_field.rs` | `on_changed`, `on_saved`, `on_reset`, `on_submitted` | event | no | as the widgets rows above |
| material (19 files) | 25: `on_pressed` ×7, `on_tap` ×4, `on_changed` ×3, `on_deleted` ×2, `on_selected`, `on_select_changed`, `on_select_all`, `on_open_changed`, `on_destination_selected`, `on_drawer_changed`, `on_end_drawer_changed`, `on_closed`, `on_submitted` | event | no | not yet audited |
| cupertino (2 files) | `on_tap`, `on_pressed`, `on_long_press` | event | no | not yet audited |

Totals: 100 event, 5 query, 105 in all. `on_key_event` is the one event callback that returns a
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

### 7. `StateCell` and `RebuildHandle` stay run-time capabilities

Neither takes `&mut Writer`. `StateCell` is already a capability bound in `init_state`
(`state_cell.rs:189`) and works unbound; `RebuildHandle` stays as ADR-0018 defines it. The guard
is extended so that `StateCell::schedule` during `build` is refused like a signal write. The
compile-time claim of this record is limited to writes into `Signal<T>`.

### 8. Order and rollback

The changes land one at a time, each with `cargo xtask check-changed` green:

1. ADR-0085 §1 (realm routing of `UiCommand::SignalWrite`), with its failing multi-presentation
   test. It does not depend on anything else here.
2. `EventCx`, `Writer`, `WriterSource`, and a `callback(|cx| ..)` helper that fixes the
   higher-ranked signature, named in the widget-author documentation.
3. The `Signal` write signatures, and removal of both `reactive()` accessors.
4. The setter signatures, migrated crate by crate by `flui migrate`, starting with a pilot on
   `flui-cupertino` and the `counter` and `todo` examples (`examples/counter.rs`,
   `examples/todo.rs`), so the pilot covers catalog code and application code. Steps 3 and 4
   land before the first crates.io publication (ADR-0091 §1).

`Writer` and `WriterSource` live in `flui-view`, beside the graph (ADR-0085 §6).

**Rollback to guard-only.** If the pilot needs explicit closure type
annotations at call sites that `callback(..)` does not cover, or the converted call sites are
materially longer than the probe's, the design switches before 1.0 to the guard-only shape: the
`Signal` handle resolves its realm itself, writes are allowed anywhere outside `build`, and the
run-time guard is the only enforcement. The pilot's diff and the decision are recorded by
amending this record.

## Alternatives considered

- **Ambient write scope (a thread-local "current writer").** Rejected: it needs the same wiring
  as `EventCx`, fails at run time (`NoScope`) instead of at compile time, and adds a thread-local
  against ADR-0097.
- **Guard only; the handle finds its realm** (the rollback shape). Not chosen now: cheapest to
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

None of these exist yet.

- `compile_fail` doctests: `signal.set(..)` inside `build` does not compile; a `&mut Writer` does
  not escape into a `'static` closure; `build` has no method returning a `Writer`;
  `WriterSource` is `!Send` (`static_assertions::assert_not_impl_any!`).
- A test that a widget invoking a user callback synchronously inside its own `build`, through a
  captured `WriterSource`, is refused by the guard (the probe
  `a_nested_sync_callback_during_build_still_needs_guard`, moved into the repository).
- A test that `AnimatedSize` no longer calls `on_end` from `build`, which fails on today's code
  (`animated_size.rs:201-209`).
- A test that `StateCell::schedule` during `build` is refused.
- ADR-0085's multi-presentation routing test, now written through `WriterSource`.
- The completed §6 table, reproduced by its grep command, in the change that alters setter
  signatures; `flui migrate` applied to `examples/` with `cargo xtask check-changed` green.
