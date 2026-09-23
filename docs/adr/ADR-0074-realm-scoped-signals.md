# ADR-0074: Realm-scoped signals as the canonical application-state layer

Status: **Accepted (2026-09-22) for `Signal<T>` and the reader registry** — shipped behind
`flui-view` feature `signals`, go/no-go measured in §8.1. Derived values and effects
(`Computed<T>`, `Effect`) were part of the first prototype and are **not** part of this
decision: the adversarial review found them unsound in four ways, and they are designed
separately in [ADR-0075](ADR-0075-derived-state-and-effects.md) (Proposed).
Two limits are part of the decision: a value read by hundreds of cells is an
`InheritedView` + field-mask (#1090) concern, not a per-cell signal (§5.10); structural
list changes are `Memo<V>`/`can_update`'s job (C1, issue #1251). The #1090 field-mask
registry (epic A4) is the next step on the same seam, **not a condition of this ADR**.
Supersedes: FOUNDATIONS C1 (signals clause), ADR-0008 §"signals-as-default are rejected"
— by the beta-roadmap mandate that locked contracts are revisable explicitly (AGENTS.md
ADR policy). The C1 text in `docs/FOUNDATIONS.md` is rewritten to §7's wording and ADR-0008
carries a `Superseded-by: ADR-0074` note.

## 1. Context

### 1.1 The gap the framework user feels

Flutter ships `setState`, `InheritedWidget`, `ValueNotifier` and nothing above them.
Application state went to the ecosystem — Provider, Riverpod, Bloc, GetX, MobX — and the
result is a split: every Flutter app of any size picks one, teaches every contributor its
vocabulary, and pays a rebuild tax the framework can neither see nor optimise (the
official options page lists only the three low-level built-ins and points at pub.dev for
everything else; `docs/research/state-model-2026.md` §8). `setState` has no granularity:
it "schedule[s] a build for this State object" — the whole `build` of that `State` —
and the only escape is to slice the widget tree by hand until each `State` is small.

FLUI today is at exactly Flutter's floor:

| Primitive | Where | Granularity |
|---|---|---|
| `ViewState::set_state_scheduled` | `crates/flui-view/src/element/unified.rs:447` | the whole element (`mark_dirty` + push on the depth heap) |
| `ValueNotifier<T>` / `ChangeNotifier` + `RebuildHandle` | `crates/flui-foundation/src/notifier.rs:368`, `crates/flui-view/src/owner/rebuild_handle.rs:89` | one listener closure per element; the closure schedules the element (`crates/flui-material/src/switch.rs:253-259` is the canonical idiom) |
| `InheritedView` + `depend_on` | `crates/flui-view/src/context/build_context.rs:490`, `crates/flui-view/src/element/behavior.rs:1300` | per element, per provider **type**, all-or-nothing (issue #1090) |

None of these is an application-state model. They are the wiring an application-state
model is built on.

### 1.2 What "canonical application state" has to solve — the 3-screen app

Take the smallest app that exposes every problem: three screens (list, detail, settings),
a form on the detail screen (20 fields, validation, dirty tracking), a list of 10 000 rows
on the list screen, and a settings value (say, a unit preference) read by cells on all
three screens.

The state layer must answer, without the author slicing widgets for performance:

1. **Where does state live that outlives a screen?** Not in a `ViewState` — navigation
   disposes it. Not in a process global — ADR-0027 forbids it (two windows are two realms).
   It lives in something the *realm* owns and screens borrow.
2. **Who rebuilds when one form field changes?** The one `TextField` reading that field and
   the "Save" button reading `form.is_dirty()`. Not the form container, not the other
   19 fields. With `setState` on a `FormState`, all 20 rebuild.
3. **Who rebuilds when row 4 217 of the list changes?** That row's cell. Not the list, not
   the sliver, not the 40 visible siblings.
4. **Who rebuilds when the unit preference flips?** Every cell that *read* it, on every
   screen that is mounted — and nobody else. Today an `InheritedView<Settings>` rebuilds
   every dependent for any field of `Settings` (#1090).
5. **Derived values** (total of the visible rows, "3 of 20 fields invalid") recompute once
   per change, not once per reader.
6. **Writes from outside the UI thread** (a network reply, a file watcher) land on the
   realm's owner thread at a frame boundary, never in the middle of a build
   (ADR-0027 enqueue-and-wake).
7. **Tests** read and write the same state through the headless driver without mounting a
   UI around it.

Every ecosystem answer to (1)–(5) — Riverpod providers, Bloc streams, MobX observables — is
a signal graph with a different spelling. The market survey (§2) shows the Rust frameworks
and both native platforms converged on the same shape: fine-grained reactive nodes whose
readers are tracked per read.

### 1.3 Why the retained three-tree model changes the design (and why it is not Solid)

Solid attaches readers to DOM expressions and Leptos to reactive nodes, so a write re-runs
exactly those; Dioxus marks the *component* that read the signal dirty and re-renders it,
which is the closest analogue to FLUI's Element. FLUI's unit of rebuild is the **Element**
(FOUNDATIONS C1: "the smallest sound invalidation unit stays the Element"): a rebuild is
`View::build` for that element followed by keyed reconciliation of its children
(`BuildOwner::build_scope`, `crates/flui-view/src/owner/build_owner.rs:57`). Signals do not
replace that; the only thing a signal can *do* to the tree is put an element on the dirty
heap. The design below therefore keeps every invariant the rebuild machinery already has
(depth-ordered drain, mid-drain re-entry budget, `catch_unwind` around `build`) and adds
exactly one thing: a **precise reader set** per signal, so that the heap receives the
readers of the value that changed and nothing else.

That is also what makes it compatible with `Memo<V>`/`can_update` (C1's one addition): a
rebuilt element whose children's `View` configs are unchanged still short-circuits at the
child boundary. Signals decide *which* elements enter the drain; memoisation decides how
far a drain propagates. The two compose.

## 2. Market (summary; sources and quotes in `docs/research/state-model-2026.md`)

| System | Node ownership | Read tracking | Write → what re-runs | Fits a retained Element tree? |
|---|---|---|---|---|
| Solid.js | owner tree (`createRoot`), disposed with owner | synchronous, during execution | the exact computations that read | model yes, granularity below Element is meaningless for us |
| Leptos `reactive_graph` | **process-global** slotmap arena (`OnceLock<RwLock<SlotMap>>`; thread-local only under `sandboxed-arenas`) + an `Owner` tree for disposal | `ReactiveNode` graph, three-state dirty/check/clean | subscribers; `Memo` re-checks before notifying | algorithm yes; the global arena is the a57b4140 mistake (ADR-0027) — take the graph, not the storage |
| Dioxus 0.7 | `generational-box` slots owned by component scope; `Signal: Copy` | `ReactiveContext` per component | marks the *component* dirty → one component re-render | closest analogue: their component ≈ our Element |
| Floem | `floem_reactive` (own fork of leptos_reactive) | same | effect calls `id.request_layout()/request_paint()` on a retained view id | yes — retained tree + signals is proven to coexist |
| Xilem | **no signals**; one app-state value, rebuild whole view tree, diff (`View::rebuild`), `Memoize` | none | everything, then diff | the C1 model; correct but the "10 000 rows" case is a full diff per keystroke |
| SwiftUI `@Observable` | per object; `ObservationRegistrar` | per **property** access inside `withObservationTracking` | the views that read those properties | yes; property granularity = our field masks |
| Compose snapshot state | `mutableStateOf` cells; snapshot isolation | per read, recorded into the current `RecomposeScope` | invalidate that scope only | yes; `RecomposeScope` ≈ Element; `derivedStateOf` ≈ `Memo` |

Convergence: **read-is-subscribe** at the smallest addressable unit (property/field), an
**owner** that bounds lifetime (scope/owner/realm), and a **memo** node that stops
propagation when its output is unchanged. Xilem is the lone dissenter and is the model C1
adopted; it trades granularity for simplicity and relies on diffing cost staying low.

## 3. Prior FLUI experience: `flui-reactivity` (`git show a57b4140`, removed in `38620127`)

The crate was a React-Hooks port: `Signal<T>`, `use_memo`, `use_effect`, `use_callback`,
a hook index reset per render (`context.rs:189` "Begin rendering a component, resetting
hook index"), a `thread_local!` `DEPENDENCY_TRACKER` and `COMPUTATION_STACK`
(`computed.rs:17,549`), batching state in thread-locals (`batch.rs:25-38`), and a
`OnceLock` global context provider (`context_provider.rs:10,77`). The removal commit's
reasoning stands verbatim: "the crate's process-global SIGNAL_RUNTIME predates the
UiRealm model (cross-realm bleed if ever wired). Any future signals story is a new
realm-scoped design, not a revival."

The full picture from the last revision before removal (`c534ea0f`, 2025-11-18 →
2026-07-28, 23 source files, 101 `#[test]` fns, plus a `BENCHMARK_RESULTS.md` that is
actually an unrelated render-object spec): one `SignalRuntime::global()` backed by
`DashMap`, every value an `Arc<Mutex<T>>` (`runtime.rs`, carrying a `PORT-CHECK-OK-SP6`
waiver), every callback `Send + Sync`, `Computed<T>` holding its own
`Mutex<HashSet<SignalId>>` dependency set, an `EffectScheduler` with `Arc<Mutex<Box<dyn
FnMut>>>` callbacks and priorities, a leptos-style `Owner` tree also built on `Mutex`es,
`HookContext` with `begin_component`/hook-index bookkeeping, `batch()` on thread-locals,
and an `async.rs` bridging to tokio channels. It depended on `any_spawner` and
`send_wrapper` that the workspace never declared, so it did not build at removal time.
**It was never wired to an Element**: no file in the crate mentions `Element`,
`BuildContext` or `mark_needs_build`, and no other crate ever imported it after the
2025-12 view rewrite — the "components re-render" in its docs referred to a hook context
of its own, not to FLUI's tree. There is nothing to salvage structurally; the reusable
parts are ideas (`Copy` handles, owner-tree cleanup, batching) that the market survey
sources better.

What must be different, point by point:

| a57b4140 | This ADR |
|---|---|
| process/thread-global runtime, tracker and batch state | every node is owned by one `UiRealm`; the tracker is a field of the build owner, not a static |
| hook order is the identity of a value (`use_state` index) | a signal is a value with a handle; no call-order contract (§5.7) |
| callbacks `Send + 'static`, `Mutex` inside | realm-affine `!Send` cells (ADR-0002 / ADR-0027 control plane); cross-thread writes are a `UiCommand` |
| effects scheduled by the signal write itself | no effects in this ADR; ADR-0075 puts them in a named phase of the binding's frame |
| independent of the Element tree | the only side effect on the tree is `schedule_build_for(element)` |

## 4. Decision

Add a realm-owned reactive graph — `Signal<T>` values in a generational arena plus a reader
registry — to the view layer, where **reading a signal inside `build` registers the
building element as a reader**, and **writing a signal schedules exactly the reader
elements** on the existing dirty heap. `setState`, `ValueNotifier`, `InheritedView` remain.
Derived values and effects are ADR-0075's subject.

## 5. Design

### 5.1 Types

```rust
/// A realm-owned cell. `Copy` handle (graph id + slot index + generation), like
/// Dioxus/Leptos; the value lives in the realm's arena, so the handle is `'static`
/// and cheap to capture in closures without `Rc` cycles. `!Send` (realm-affine).
pub struct Signal<T: 'static> { slot: SignalSlot, .. }

impl<T> Signal<T> {
    // build-time reads: the building element becomes a reader
    pub fn get(self, cx: &dyn BuildContext) -> T where T: Clone;           // panics on a stale handle
    pub fn with<R>(self, cx: &dyn BuildContext, f: impl FnOnce(&T) -> R) -> R;
    pub fn try_get(self, cx: &dyn BuildContext) -> Result<T, SignalError>;  // stale = Err(Released)
    pub fn try_with<R>(self, cx: &dyn BuildContext, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
    // reads/writes outside build: callbacks, tests, realm commands
    pub fn peek<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
    pub fn set(self, r: &Reactive, value: T) -> Result<(), SignalError>;   // marks readers, equal or not
    pub fn update<R>(self, r: &Reactive, f: impl FnOnce(&mut T) -> R) -> Result<R, SignalError>;
    pub fn set_if_changed(self, r: &Reactive, value: T) -> Result<bool, SignalError> where T: PartialEq;
    pub fn detach(self) -> SignalSender<T>;                                 // the Send form (§5.8)
}
```

- **Creation is owned.** The canonical idiom is `cx.signal(value)` from
  `ViewState::init_state` (or `did_change_dependencies`): the slot is owned by that
  element and released when it unmounts, so a widget cannot leak slots. Application-level
  state that outlives every screen uses `Reactive::signal(value)`, released with the realm;
  `Reactive::signal_owned_by(element, value)` is the explicit form of the former. All
  three have `try_` variants; creating a slot **inside `build`** is refused
  (`SignalError::CreatedDuringBuild`, one slot per rebuild would be a leak).
- `Reactive` is the realm's reactive graph, reachable from `BuildContext` (`cx.reactive()`)
  in every lifecycle hook and callback, and from the headless driver. It is `!Send + !Sync`,
  lives next to `BuildOwner` in the realm's services, and is dropped with the realm. Every
  graph has a process-unique id that its slots carry, so a handle used against another
  realm's graph is `SignalError::ForeignGraph`, never a read of someone else's value.
- **No trait bound on `T` beyond `'static`.** `set` and `update` mark readers **always**,
  whether or not the value changed — there is no equality check and no `PartialEq` bound.
  `set_if_changed` (requires `T: PartialEq`) is the opt-in that skips marking on an equal
  write. This is the rule, not a default (§5.7); it is what keeps C1's Druid warning
  honoured.
- **Handles are `PartialEq`/`Eq` by identity** (issue #1251 covers what a view does with
  that for `can_update`).
- **Naming.** The derived value planned in ADR-0075 is `Computed<T>`, not `Memo<T>`:
  `flui_view::Memo<V>` is already C1's view-memoization combinator in the same crate and
  the two collided in the first compile. The market is split (Leptos/Solid/Dioxus `Memo`,
  Vue `computed`, Compose `derivedStateOf`), so `Computed` is a recognised name.

### 5.2 Read-is-subscribe through the existing `BuildContext`

This is the one place the design touches the build seam, and it deliberately reuses the
seam `depend_on` already has:

- `ElementBuildContext` (`crates/flui-view/src/context/element_build_context.rs`) knows
  the building `ElementId`. `Signal::get(cx)` calls `cx.signal_read(slot)`, which appends
  `slot → element_id` to the reader set and returns the value (the depth the dirty heap
  needs is resolved by the inbox at drain time, as for every external schedule). This is *exactly*
  what `depend_on_inherited` does for a provider (`element_build_context.rs:267`), with a
  signal slot in place of a provider node.
- Reader sets are **cleared and rebuilt on every build of that element** (Compose's and
  Solid's rule): a build that no longer reads a signal stops depending on it. Storage:
  per signal a `SmallVec<ElementId>` (rebuilt readers are re-added in build order; dedup on
  insert), per element a `SmallVec` of slot indices for the clear step. Both are linear
  today; issue #1248 tracks the slab-indexed form.
- **Trigger #22 is not touched.** `Signal::get` is not a *capability acquisition*; it is a
  read, the same class as `depend_on`. The scanner's token list stays as it is, and the
  ADR adds one new refusal instead: `Signal::set`/`update` or a slot creation inside a guarded
  body (`build`/`perform_layout`/`paint`/…) is a violation, because a write during build
  is the unbounded-loop hazard #22 exists for. `peek` is allowed anywhere.
- Reads outside build (callbacks, `init_state`, effects) do not subscribe an element —
  there is none building. They go through `peek` (a read that registers nothing).

### 5.3 Write → dirty exactly the readers

```text
signal.set(v)                         // owner thread, outside build
  └─ reactive.mark(slot)
       └─ for elem in readers[slot]: ExternalBuildScheduler::schedule(elem, RebuildReason::SignalChange)
```

- `schedule` is the existing entry the `RebuildHandle` uses: it lands in the owner's
  external inbox and requests a frame; the depth heap and the `MAX_MID_DRAIN_ABSORBS`
  budget (issue #1180) apply unchanged at drain. A signal cannot create a rebuild path
  the framework does not already have.
- Writes are coalesced per frame by construction: the inbox dedups by element, and a
  second write before the drain finds the element already queued. One write with R readers
  is R inbox inserts today (issue #1249: batch under one lock).
- **Re-entrancy.** A read or write closure runs with no borrow of the graph held: the
  value is taken out on loan, the closure runs, the value is put back if the slot is still
  the same generation. `a.with(cx, |_| b.get(cx))` and `a.update(&r, |v| *v += b.peek(..))`
  are fine; touching the *same* slot from its own closure is `SignalError::Reentrant`,
  never a `RefCell` panic.
- Writes during a drain (from `did_update_view`, for example) fall into the mid-drain
  absorb path; writes from `build` are refused (§5.2).

### 5.4 Effects

Deferred to [ADR-0075](ADR-0075-derived-state-and-effects.md) together with derived values.
The prototype's effects phase ran only in the headless harness, never in the product frame
(`draw_frame_impl`), which is the first of ADR-0075's requirements. Until then, side effects
run from callbacks, `did_update_view`, or realm commands.

### 5.5 One scheduler and one discipline, two registries: `InheritedView` field masks (#1090)

The reader set keyed by `SignalSlot` is the same *kind* of edge #1090 needs keyed by
`(provider TypeId, field bit)`: a dependent recorded with what it read, notified only when
that changed. Epic **A4** lands the field-mask half on the inherited path with the same
scheduler (`schedule_build_for`, `RebuildReason::DependencyChange`) and the same discipline
(re-derive the read set on every build, below) — but in its own registry
(`InheritedBehavior::dependents` + the reverse index in `InheritedDependencies`), not the
signal arena. Structural unification of the two registries is #1254's question, not this
section's claim:

- `FieldMask<D>` — a 64-bit field set **typed by the provider's data type** — and the
  opt-in `#[derive(InheritedData)]` (one `FIELD_<NAME>: FieldMask<Self>` constant per field
  plus `field_mask_diff`). `depend_on_field::<T, _>` takes `FieldMask<T::Data>`, so a
  selector of another data type is a compile error rather than a silently wrong
  subscription (a `compile_fail` doctest pins it); element storage and the object-safe
  context method carry the untyped `FieldSet`; the lowering (`FieldMask::erase`) and the
  recording helper are crate-private, so outside `flui-view` a `FieldSet` is only `NONE`/`ALL`
  and the typed selector cannot be bypassed (a second `compile_fail` doctest pins it). The
  marker is invariant in `D` (`PhantomData<fn(D) -> D>`), so subtyping cannot coerce one data
  type's mask into another's, and `BuildContext` is sealed so a downstream context cannot
  forward an erased set to a different provider `TypeId`. Market
  check: Compose's `derivedStateOf`/`snapshotFlow` and SwiftUI's `@Observable` track reads
  per property with no untyped selector at all; a typed mask is the closest static shape
  that keeps one provider type per `TypeId` lookup;
- `InheritedView::changed_fields(old)` — `ALL`/`NONE` by default from
  `update_should_notify`, per-field for a provider whose data implements `InheritedData`;
- `InheritedBehavior::dependents` stores `DependentEntry { depth, mask, lifecycle_mask }`;
  `on_view_updated` schedules a dependent only if `mask | lifecycle_mask` intersects the
  changed set. `depend_on::<T, _>` records `FieldSet::ALL` through the same path as
  `depend_on_field`: whole-provider parity is the degenerate mask, not a second mechanism;
- `MediaQuery::size_of(cx)` / `text_scale_factor_of` / … and `Theme::color_scheme_of` /
  `text_theme_of` (plus the general `depend_on_fields(cx, mask, f)`) are the field
  accessors; `MediaQuery::of` / `Theme::of` keep the whole-provider dependency.

**Mapping decision — reset-on-build (deliberate divergence from Flutter).** Flutter's
`Element._dependencies` and `InheritedElement._dependents` accumulate from the first
`dependOnInheritedElement` until unmount: a widget that read `MediaQuery.sizeOf` once keeps
rebuilding on size changes even after it stopped reading it. Here the fields an element is
recorded as reading at each of its providers are those of its **latest** build: in the
build drain (`BuildOwner::drain_build_scope`, right after the element's `build` — the
signal registry re-derives its reader set in the same build) the element's masks at its previous providers reset to `NONE`, the
build's reads re-accumulate from the dependency sink, and a provider entry still at `NONE`
afterwards is removed (and dropped from the reverse index). The `LayoutBuilder`-scoped
drain goes through the same function, so it does the same for the elements it builds. The signal registry got the same rule in §5.1;
`a_rebuild_re_derives_the_field_set_so_a_dropped_read_stops_depending` in
`media_query_fields.rs` pins it (read `size` in the first build, `text_scale_factor` in the
second → a later size-only change rebuilds nothing). Cost: one hash lookup per previous
provider per build; benefit: no stale rebuilds from reads a conditional branch stopped making.
Reset-on-build covers reads made in **`build` only**. A read in `init_state` or
`did_change_dependencies` is recorded in a separate `lifecycle_mask` that **accumulates until
unmount**, as in Flutter. It is deliberately not re-derived per `did_change_dependencies`
call: that hook does not run on the first build after `init_state` here, so resetting on it
would drop what `init_state` read; the cost is that a conditional read in a lifecycle hook
stays subscribed until unmount (Flutter's behavior for every read). Why this matters: framework states such as `FocusState` and `DraggableState` acquire an
inherited value in a lifecycle hook and do not re-read it in `build`, and a rebuild from any
other cause must not unsubscribe them
(`a_dependency_acquired_in_a_lifecycle_hook_survives_a_rebuild_that_does_not_reread_it`).
A build that panics is not evidence of what the element reads: when this element's build is
recovered with an `ErrorView` (a flag `build_or_recover` sets on the drain's owner, not a
scan of the diagnostic panic queue), its previous masks are kept and the
sink's records are only added, and the signal registry likewise restores the previous read set
when the build unwinds — so the element stays subscribed and rebuilds once the failing
condition clears (`a_build_that_panics_before_reading_keeps_its_dependency`,
`a_build_that_unwinds_keeps_its_previous_read_set`).

#1090's acceptance tests (`crates/flui-widgets/tests/media_query_fields.rs`,
`crates/flui-material/tests/theme_fields.rs`) pin: a size-only change rebuilds the size
readers and the whole-`of` readers, not the text-scale readers; the reverse; an equal
provider swap rebuilds nobody; a field reader still rebuilds when its own field changes
after an unrelated one. `rebuild_exactness` (non-dependents never rebuild) stays.
Issue #1254 (ALT-2, `#[derive(Observable)]`) asks whether this registry should become the
*only* one with signals as its one-field case; decided before the catalog accepts
`Signal<T>` inputs.

### 5.6 What the three screens look like

**Counter** (today: `StateHandle` + `RebuildHandle`; with signals — the handle is `Copy`,
the write happens from the tap callback, outside `build`):

```rust
#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState { count: Option<Signal<u32>> }

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, cx: &dyn BuildContext) {
        self.count = Some(cx.signal(0u32));            // owned by this element,
    }                                                  // released with it
    fn build(&self, _v: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.expect("init_state ran");   // Copy
        let r = cx.reactive();                              // Rc clone; the callback owns one
        Column::new((
            Text::new(format!("{}", count.get(cx))),   // reads → this element is a reader
            Button::new("+1").on_tap(move || { let _ = count.update(&r, |c| *c += 1); }),
        ))
    }
}
```

**Todo list, 10 000 rows** — the list reads only the *row count*; each row reads its own
signal, so toggling row 4 217 rebuilds that row (measured: 2 elements, the row and its
box, §8.1):

```rust
struct Todos { count: Signal<usize>, rows: Rc<RefCell<Vec<Signal<Todo>>>> }

impl StatelessView for TodoList {
    fn build(&self, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.todos.count.get(cx);
        let rows = self.todos.rows.borrow();
        Column::new(rows.iter().take(count).map(|row| TodoRow { row: *row }.boxed()).collect())
    }
}
// TodoRow::build reads `self.row.with(cx, |t| t.done)`; a toggle is
// `row.update(&r, |t| t.done = !t.done)` from the row's tap callback.
```

**Form, 20 fields** — one signal per field; each `TextField` reads its own. The "Save"
cell reads all twenty (so it rebuilds on every keystroke, like the `setState` form) until
ADR-0075's `Computed` lands; the measured shape is in §8.1.

```rust
struct FormModel { fields: [Signal<String>; 20] }
// TextField i reads fields[i].with(cx, ..); typing rebuilds that TextField (and Save).
```

A headless test constructs the same model with `binding.reactive()` and drives it with
`fields[3].set(&r, "x".into())`; the assertion is on the readers rebuilt
(`BuildOwner::last_frame_build_report`), not on pixels.

### 5.7 Rules without hooks

- A signal is created explicitly (`cx.signal(v)` in `init_state`, or `r.signal(v)` at the
  app root) and held in a field. There is no `use_signal()` whose identity is the call
  index; `build` may read signals in any order, conditionally, or in loops.
- `build` is still a pure function of `(View, State, reads)`; the reads are recorded, not
  ordered.
- Lifetime is ownership, not call structure: a slot created through `cx.signal` is released
  when its element unmounts (the registry drops that element's reads at the same time); an
  app-level slot is released with the realm. A `Copy` handle that outlives its slot is a
  `SignalError::Released` on the next `try_*`/`peek`, and a panic through the plain
  `get`/`with` (documented under `# Panics`).
- Equality is opt-in, never implied: a plain `set` marks readers even for an equal value;
  only `set_if_changed` compares, and says so in its bounds.

### 5.8 Threads

- `Signal`, `Reactive` are `!Send + !Sync` — realm-affine like the element tree
  (ADR-0027 §"one realm, one owner"). A handle carries its graph's id, so it cannot be
  attached to another realm's graph by accident (`SignalError::ForeignGraph`).
- Cross-thread writes go through the realm proxy: `UiCommand::SignalWrite(Box<dyn
  FnOnce(&Reactive) + Send>)` (the same shape as `Navigation(NavigatorCommand)`) executes on
  the owner thread at the next Idle drain, marks readers, and wakes a frame —
  enqueue-and-wake, never touch the tree. The closure cannot capture a `Signal` (it is
  `!Send`); it captures `signal.detach()`, a `SignalSender<T>` that is `Send + Sync` and
  re-attaches with `.attach()` on the owner side. `flui-app` tests the round trip.
- Writes from a dead realm's sender return `OwnerGone` (channel identity), as every other
  realm-scoped command does.
- `UiCommand::SignalWrite` and `UiCommandSender::send_signal_write` are **internal**
  (`pub(crate)`, exercised by a realm test) until D3 (the async `Task` slice) vends a
  `SignalSender` through the realm handle; this ADR does not add public realm API.

### 5.9 Testability

`HeadlessBinding` (`crates/flui-testing/src/lib.rs`) exposes `reactive()`; a test creates
signals, mounts a tree that reads them, writes, calls `pump_frame`, and asserts (a) the
value seen by the tree, (b) **which elements rebuilt** — the prototype adds a
`RebuildReason::SignalChange` counter to the existing `flui::reconcile`/`build_scope`
telemetry (a per-frame `elements_rebuilt` figure `FrameStats` does not carry today, per the
scout of `crates/flui-devtools/src/profiler.rs`). The over-/under-rebuild assertions of
#1090's acceptance list are the same assertions, written against signals.

### 5.10 `setState`, `ValueNotifier`, `InheritedView` — what happens to them

Nothing is removed in this ADR.

- `set_state_scheduled` stays the way to mutate *widget-local* state (an expanded/collapsed
  flag, a pressed visual). Guidance: local, short-lived, read by this element only →
  `setState`; shared, long-lived, read by a handful of elements → signal (created with
  `cx.signal` by the element that owns its lifetime, or at the app root).
- **Choosing by reader count.** A signal write costs about **1.3–1.8 µs per reader** on top of
  what one `setState` on the readers' common parent costs (§8.1, 600 readers: 1.77–2.09 ms
  against 1.01 ms across two runs), because every reader is scheduled through the inbox individually.
  Below ~50 readers that is noise next to the rebuilds it saves; above it the scheduling
  itself dominates. Rule: **a value with fewer than ~50 readers is a `Signal`; a value read
  by hundreds of cells (a theme slot, a unit preference, a media-query field) is an
  `InheritedView` with field masks (#1090)** — one dependency edge per subtree, and the
  masks keep the granularity signals would have given.
- `ValueNotifier`/`ChangeNotifier` stay as the `Listenable` contract for controllers
  (`AnimationController`, `ScrollController`, `TextEditingController`,
  `WidgetStatesController`). Migrating a notifier-backed value is a listener that writes the signal
  (`notifier.add_listener(move || { let _ = sig.set(&r, notifier.value()); })`); the
  reverse is a `ValueListenableBuilder` reading a signal via `peek` in a listener. No
  adapter type ships.
- `InheritedView` stays the *scoping* mechanism (nearest-ancestor lookup) and gains field
  masks via the shared registry (§5.5). A provider whose value is a `Signal<T>` is the
  recommended way to put realm state into a subtree.

## 6. Consequences

**Positive.** Application state has a canonical, framework-visible home; rebuild scope is
decided by reads, not by widget slicing; `Copy` handles remove the listener/handle
boilerplate the material catalog repeats in `init_state` today; the #1090 registry is
built once; the headless driver tests state directly; cross-thread writes are already
covered by the realm command inbox.

**Negative / risks.**
- The reader-set clear-and-rebuild on every build costs an allocation-free but real
  bookkeeping step per read (§8 measures it).
- Two ways to hold state (`setState` and signals) is one more thing to teach; the
  guidance in §5.10 has to be in the widget-authoring docs from day one.
- A signal read in `build` that is *also* written in the same element's `did_update_view`
  is a foot-gun (mid-drain absorb budget, then a loud stop); the refusal in §5.2 covers
  `build` only, the budget covers the rest.
- Derived values and effects are not here; until ADR-0075 lands, a cell that needs a
  derived value reads every input and pays that rebuild (§5.6, form).

## 7. Amendment to FOUNDATIONS C1 (applied)

C1 currently reads "signals out … an application-author signal crate that drives
`Element::mark_needs_build` from outside the catalog is a permitted post-parity opt-in,
gated by a refusal trigger barring signal subscriptions from `build`/`layout`/`paint`."
This ADR proposes replacing that clause with: *"A realm-owned reactive graph
(`Signal` and its reader registry, ADR-0074; derived values and effects in ADR-0075)
is a first-class state layer of the view crate.
Reading a signal in `build` is the sanctioned subscription path (the same class as
`depend_on`); **writing** or **creating** a signal inside `build`/`layout`/`paint`
is refused (trigger #24, and at run time). The catalog crates may accept `Signal<T>` values as widget
inputs but never own application state."* The sentence "the smallest sound invalidation
unit stays the Element" is unchanged and is what §5.3 relies on. ADR-0008's "signals
route invalidation around the retained tree" objection is answered by §5.3: they do not;
they feed the same heap. The user mandate for the beta roadmap ("ADRs revisable
explicitly") is the authority for revisiting a locked contract; this section is the
explicit revision.

## 8. Phase-2 measurement plan and go/no-go

Prototype scope: `Signal`/`Reactive` in `flui-view` behind `feature = "signals"`,
reader registry, `RebuildReason::SignalChange`, `HeadlessBinding::reactive()`, one
`UiCommand::SignalWrite` variant. No catalog changes.

Benchmarks (criterion, headless, `just bench-signals`), each in two variants — **A**
`setState` on the owning `ViewState`, **B** signals — same widget tree:

| Scenario | Change | Measured |
|---|---|---|
| List, 10 000 rows, 40 visible | toggle one visible row | elements rebuilt, render objects re-laid-out, wall time of `pump_frame` |
| List, 10 000 rows | append one row | same |
| Form, 20 fields | type one character into field 7 | same, plus whether the Save button rebuilt |
| Form, 20 fields | field 7 goes empty→non-empty (memo flips) | same |
| Settings value read by 200 cells across 3 mounted screens | flip it | same |
| Idle | no change, 100 frames | `pump_frame` wall time (bookkeeping overhead of reader sets) |

Go if, for the four "one thing changed" scenarios, **B rebuilds ≤ 3 elements** (the reader
plus at most its memo-dependent readers) where A rebuilds the owning subtree, **relayout
count is ≤ A**, `pump_frame` wall time for B is ≤ A, and the idle overhead of B is
≤ 5 % of A's idle frame. The #1090 `Theme`/`MediaQuery` field test through the same discipline is epic A4's
acceptance, not this ADR's condition (§5.5).

No-go if the idle overhead exceeds 5 %, if any scenario needs a `PartialEq` bound on plain
`Signal<T>` to meet the rebuild count, or if the write-in-build refusal cannot be
expressed in `scripts/check-frame-capability-scope.sh` without false positives on
`peek`.

### 8.1 Phase-2 results (2026-09-22, re-measured after the Computed/Effect split)

`just bench-signals` (`crates/flui-widgets/benches/signals_rebuilds.rs`), M1 8-core /
8 GB, `CARGO_BUILD_JOBS=6`, `CARGO_INCREMENTAL=0`, warm shared target, **dev profile**
(workspace at opt-level 1, dependencies at 2 — the release profile would have meant a
cold build of the GPU stack inside the build slot), one run, criterion 2 s measurement.
Counts come from `BuildOwner::last_frame_build_report` and the frame's difference of
`PipelineOwner::layout_roots_total`; both variants mount the same tree shape (two elements
per cell: a stateless cell view over a `SizedBox`). The form's "Save" cell reads all 20
fields in both variants (no derived value: that is ADR-0075's subject), so a keystroke
rebuilds the field's reader **and** the Save cell under signals. Counts do not depend on
the profile; timings are dev-profile and only comparable A against B.

| scenario | variant | elements built | by reason | layout roots | change + frame |
|---|---|---:|---|---:|---:|
| list 10k: one row changes | A setState | 20003 | parent_update=20002 state_change=1 | 1 | 15.55 ms |
| list 10k: one row changes | **B signals** | **2** | parent_update=1 signal_change=1 | 1 | **4.28 ms** |
| list 10k: append one row | A setState | 20003 | initial_mount=2 parent_update=20000 state_change=1 | 2 | 33.98 ms |
| list 10k: append one row | B signals | 20005 | initial_mount=2 parent_update=20002 signal_change=1 | 2 | 35.57 ms |
| form 20: keystroke in one field | A setState | 44 | parent_update=43 state_change=1 | 1 | 37.87 µs |
| form 20: keystroke in one field | **B signals** | **4** | parent_update=2 signal_change=2 | 1 | **18.26 µs** |
| setting read by 3×200 cells | A setState | 1209 | parent_update=1208 state_change=1 | 1 | 1.01 ms |
| setting read by 3×200 cells | B signals | 1200 | parent_update=600 signal_change=600 | 1 | 1.77 ms |
| idle frame (list 10k mounted) | A setState | 0 | – | 0 | 4.11 µs |
| idle frame (list 10k mounted) | B signals | 0 | – | 0 | 4.14 µs |

The reverted prototype's derived-value scenario ("form 20: field flips validity", a
`Computed` invalid count read by the Save cell) measured 4 elements / 17.9 µs against
44 / 39.2 µs; that figure is ADR-0075's baseline to beat, not a claim of this ADR.

Reading against the go/no-go of §8:

- **"One thing changed" rebuilds exactly the readers.** List row: 2 elements instead of
  20 003; form keystroke: 4 (the field's reader and the Save cell that reads every field,
  each with its `SizedBox` child) instead of 44. The §8 wording "≤ 3 elements" counted
  readers, not their children; the measured figure is *2 elements per reader*, which is
  the tree shape, not a signals cost. **Met.**
- **Relayout count ≤ A.** Equal in every scenario (the changed box is the only layout
  root either way). **Met.**
- **Wall time B ≤ A** where the readers are few: 3.6× (list) and 2.1× (form) faster.
  **Met.** Where *everyone* reads the value (600 cells): B rebuilds the same 1 200
  elements but takes 1.77 ms against 1.01 ms — scheduling 600 readers through the inbox
  (a `HashMap` insert and a heap push each) costs about 1.3 µs per reader more than one
  `setState` on their common parent (2.09 ms / 1.8 µs in the first run; issue #1249). **Not met for fan-out writes**; this is the price of
  precision and the honest guidance is §5.10's: a value every cell reads belongs in an
  `InheritedView` (one dependency edge per subtree, field masks per #1090), not in a
  signal each cell reads. Append: B is not better (36.4 vs 34.6 ms, within noise): the
  list root re-emits every child and the framework rebuilds all of them
  (`parent_update=20 002`) — signals decide *which* elements enter the drain, and here the
  parent legitimately does; stopping the propagation at unchanged children is
  `Memo<V>`/`can_update`'s job (C1), orthogonal to this ADR and now measurable with the
  same telemetry.
- **Idle overhead ≤ 5 %.** 4.14 vs 4.11 µs: 10 000 registered readers cost nothing per
  idle frame. **Met.**

**Verdict: go**, with two written-down limits: (1) fan-out values are an `InheritedView`
concern, not a per-cell signal read; (2) structural list changes need `Memo<V>`/
`can_update` regardless of the state model.

## References

- FOUNDATIONS C1, C5, C8 (`docs/FOUNDATIONS.md:89-124`); ADR-0008 §2–§3
  (`docs/adr/ADR-0008-flui-view-leapfrog-buildcontext-inherited-element.md`); ADR-0018;
  ADR-0027 (`UiRealm`, `UiCommandSender`); PORT.md trigger #22; issue #1090.
- `crates/flui-view/src/context/build_context.rs:490` (`depend_on`),
  `element_build_context.rs:267` (`depend_on_inherited`), `element/behavior.rs:1300`
  (`dependents`), `owner/rebuild_handle.rs:89,133`, `element/unified.rs:426-460`,
  `crates/flui-app/src/app/ui_realm.rs:231,302`.
- `git show a57b4140` (flui-reactivity), `git show 38620127` (removal).
- `docs/research/state-model-2026.md` — market survey with sources.
- ADR-0075 (Proposed) — derived values and effects; follow-ups #1248–#1254.
