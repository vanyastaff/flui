# ADR-0074: Realm-scoped signals as the canonical application-state layer

- **Status:** Accepted — for `Signal<T>` and the reader registry, always compiled (the `signals`
  feature was removed by ADR-0085 §5). Derived values and effects (`Computed<T>`, `Effect`) are
  not part of this decision; they are designed in [ADR-0075](ADR-0075-derived-state-and-effects.md).
- **Date:** 2026-09-22
- **Supersedes:** the signals clause of FOUNDATIONS C1 (now §7's wording) and ADR-0008's
  "signals-as-default are rejected" (ADR-0008 has since been retired).
- **Amended-by:** [ADR-0085](ADR-0085-reactive-core-placement-and-phase-subscribers.md)
  (Proposed; its §1, §2 and §5 ship ahead of acceptance) — its §1 routes a cross-thread write by
  the slot's graph and replaces §5.8's `SignalWrite` command shape; its §2 moves the handles and
  the read parameter into `flui_foundation::read_scope` (reads take any `&S` where
  `S: ReadScope + ?Sized`, which `BuildContext` is, instead of `&dyn BuildContext`, and writes
  go through `SignalWriteExt`); its §5 removes the `signals` feature. The write signature is
  ADR-0086's.

Two limits are part of the decision: a value read by hundreds of cells is an `InheritedView` +
field-mask concern, not a per-cell signal (§5.10); structural list changes are
`Memo<V>`/`can_update`'s job (C1, #1251).

## 1. Context

### 1.1 The gap

Flutter ships `setState`, `InheritedWidget`, `ValueNotifier` and nothing above them; application
state went to the ecosystem (Provider, Riverpod, Bloc, GetX, MobX), so every sizeable app picks
one and pays a rebuild tax the framework cannot see. `setState` has no granularity — it rebuilds
the whole `State` — and the only escape is slicing widgets by hand.

FLUI was at exactly that floor: `ViewState::set_state_scheduled` (whole element),
`ValueNotifier`/`ChangeNotifier` + `RebuildHandle` (one listener closure per element), and
`InheritedView` + `depend_on` (per element, per provider type, all-or-nothing, #1090). These are
wiring, not an application-state model.

### 1.2 What canonical application state must solve

A three-screen app (list of 10 000 rows, a 20-field form, a settings value read on all screens)
must answer, without the author slicing widgets for performance:

1. **Where does state that outlives a screen live?** Not in a `ViewState` (navigation disposes
   it), not in a process global (ADR-0027: two windows are two realms). In something the realm
   owns and screens borrow.
2. **One form field changes** → rebuild that field and the "Save" button, not all 20.
3. **Row 4 217 changes** → that row, not the list or its visible siblings.
4. **The setting flips** → every cell that read it, on every mounted screen, and nobody else.
5. **Derived values** recompute once per change, not once per reader.
6. **Off-thread writes** (network, file watcher) land on the owner thread at a frame boundary,
   never mid-build (ADR-0027 enqueue-and-wake).
7. **Tests** read and write the state through the headless driver.

Every ecosystem answer to (1)–(5) is a signal graph with a different spelling.

### 1.3 Why the retained tree shapes the design

Solid and Leptos re-run the exact reactive computations that read; Dioxus marks the *component*
dirty — the closest analogue to FLUI's Element. FLUI's unit of rebuild stays the Element
(C1): a rebuild is `View::build` plus keyed reconciliation. The only thing a signal does to the
tree is put an element on the dirty heap. The design keeps every invariant of the rebuild
machinery (depth-ordered drain, mid-drain absorb budget, `catch_unwind` around `build`) and adds
one thing: a **precise reader set** per signal.

That composes with `Memo<V>`/`can_update`: signals decide *which* elements enter the drain;
memoisation decides how far a drain propagates.

## 2. Market (sources in `docs/research/state-model-2026.md`)

| System | Node ownership | Write → what re-runs | Take-away |
|---|---|---|---|
| Solid.js | owner tree | exact computations that read | granularity below Element is meaningless here |
| Leptos `reactive_graph` | process-global arena + `Owner` tree | subscribers; `Memo` re-checks | take the graph algorithm, not the global storage |
| Dioxus 0.7 | `generational-box` slots per component scope; `Signal: Copy` | the component | closest analogue |
| Floem | fork of leptos_reactive | retained view requests layout/paint | retained tree + signals coexist |
| Xilem | no signals; rebuild and diff, `Memoize` | everything, then diff | the C1 model; a full diff per keystroke at 10 000 rows |
| SwiftUI `@Observable` | per object | views that read the property | property granularity ≈ field masks |
| Compose snapshot state | `mutableStateOf` cells | the reading `RecomposeScope` | `RecomposeScope` ≈ Element |

Convergence: read-is-subscribe at the smallest addressable unit, an owner that bounds lifetime,
and a memo node that stops propagation on unchanged output.

## 3. Prior FLUI experience: `flui-reactivity` (removed in `38620127`)

A React-Hooks port: a process-global `SignalRuntime` on `DashMap`, every value an
`Arc<Mutex<T>>`, hook-index identity, thread-local dependency tracking and batching, `Send + Sync`
callbacks. It never built at removal time and was never wired to an Element. The removal
rationale stands: its global runtime predates `UiRealm` and would bleed across realms.

| Old crate | This ADR |
|---|---|
| process/thread-global runtime | every node owned by one `UiRealm`; the tracker is a field of the build owner |
| hook order is identity | a signal is a value with a handle; no call-order contract (§5.7) |
| `Send + 'static` callbacks, `Mutex` inside | realm-affine `!Send` cells; cross-thread writes are a `UiCommand` |
| effects scheduled by the write | no effects here (ADR-0075) |
| independent of the tree | the only side effect is scheduling a reader element |

## 4. Decision

Add a realm-owned reactive graph — `Signal<T>` values in a generational arena plus a reader
registry — to the view layer. **Reading a signal inside `build` registers the building element as
a reader; writing a signal schedules exactly the reader elements** on the existing dirty heap.
`setState`, `ValueNotifier` and `InheritedView` remain.

## 5. Design

### 5.1 Types

```rust
/// Realm-owned cell. `Copy` handle (graph id + slot index + generation); `!Send`.
pub struct Signal<T: 'static> { /* SignalSlot */ }

impl<T> Signal<T> {
    // build-time reads: the building element becomes a reader
    pub fn get(self, cx: &dyn BuildContext) -> T where T: Clone;  // panics on a stale handle
    pub fn with<R>(self, cx: &dyn BuildContext, f: impl FnOnce(&T) -> R) -> R;
    pub fn try_get(self, cx: &dyn BuildContext) -> Result<T, SignalError>;
    pub fn try_with<R>(self, cx: &dyn BuildContext, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
    // outside build: callbacks, tests, realm commands
    pub fn peek<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
    pub fn set(self, r: &Reactive, value: T) -> Result<(), SignalError>;  // marks readers, equal or not
    pub fn update<R>(self, r: &Reactive, f: impl FnOnce(&mut T) -> R) -> Result<R, SignalError>;
    pub fn set_if_changed(self, r: &Reactive, value: T) -> Result<bool, SignalError> where T: PartialEq;
    pub fn detach(self) -> SignalSender<T>;                                // the Send form (§5.8)
}
```

`SignalError` is `Released`, `ForeignGraph`, `WrittenDuringBuild`, `CreatedDuringBuild`,
`Reentrant` (`#[non_exhaustive]`).

- **Creation is owned.** The idiom is `cx.signal(value)` from `init_state` (or
  `did_change_dependencies`): the slot is owned by that element and released when it unmounts.
  App-level state uses `Reactive::signal(value)`, released with the realm;
  `Reactive::signal_owned_by(element, value)` is the explicit form. All have `try_` variants.
  Creating a slot inside `build` is `SignalError::CreatedDuringBuild` (one slot per rebuild would
  leak).
- **`Reactive`** is the realm's graph, reachable as `cx.reactive()` and from the headless driver;
  `!Send + !Sync`, lives beside `BuildOwner`, dropped with the realm. Each graph has a
  process-unique id carried by its slots, so a handle used against another realm is
  `SignalError::ForeignGraph`, never a read of someone else's value.
- **No bound on `T` beyond `'static`.** `set`/`update` always mark readers; `set_if_changed`
  (`T: PartialEq`) is the opt-in that skips an equal write (§5.7).
- **Handles compare by identity** (#1251 covers `can_update`).
- **Naming.** ADR-0075's derived value is `Computed<T>`, because `flui_view::Memo<V>` is already
  C1's view-memoization combinator.

### 5.2 Read-is-subscribe through the existing `BuildContext`

- `ElementBuildContext` knows the building `ElementId`. `Signal::get(cx)` subscribes it,
  appending `slot → element` to the reader set — exactly what `depend_on_inherited` does for a
  provider, with a signal slot in place of a provider node. (ADR-0085 §2 routes this through
  the context's `ReadScope` and a sink bound to the element; the method `cx.signal_read(slot)`
  it replaced is gone.)
- Reader sets are **cleared and rebuilt on every build of that element** (Compose's and Solid's
  rule): a build that no longer reads a signal stops depending on it. A build that unwinds
  restores the previous read set. Storage is linear small-vectors per signal and per element
  today (#1248 tracks the slab-indexed form).
- A read is not a capability acquisition; it is the same class as `depend_on` and is allowed in
  `build`. **Writing or creating a signal while an element is building is refused at run time**
  (`SignalError::WrittenDuringBuild` / `CreatedDuringBuild`): a write during build is the
  unbounded-rebuild-loop hazard. The run-time guard is authoritative (ADR-0078). `peek` is allowed
  anywhere.
- Reads outside build subscribe nothing — there is no building element — and go through `peek`.

### 5.3 Write → dirty exactly the readers

```text
signal.set(v)                         // owner thread, outside build
  └─ reactive.mark(slot)
       └─ for elem in readers[slot]: schedule(elem, RebuildReason::SignalChange)
```

- `schedule` is the entry `RebuildHandle` already uses: the owner's external inbox plus a frame
  request; the depth heap and the mid-drain absorb budget (#1180) apply unchanged. A signal cannot
  create a rebuild path the framework does not already have.
- Writes coalesce per frame: the inbox dedups by element. One write with R readers is R inbox
  inserts (#1249: batch under one lock).
- **Re-entrancy.** A read/write closure runs with no borrow of the graph held (the value is on
  loan, put back if the generation still matches). Touching *another* slot from inside is fine;
  touching the same slot is `SignalError::Reentrant`, never a `RefCell` panic.
- Writes during a drain (e.g. from `did_update_view`) fall into the mid-drain absorb path.

### 5.4 Effects

Deferred to [ADR-0075](ADR-0075-derived-state-and-effects.md) with derived values. Until then,
side effects run from callbacks, `did_update_view`, or realm commands.

### 5.5 One scheduler and one discipline, two registries: `InheritedView` field masks (#1090)

The reader set keyed by `SignalSlot` is the same kind of edge #1090 needs keyed by
`(provider TypeId, field bit)`. The field-mask half uses the same scheduler
(`RebuildReason::DependencyChange`) and the same re-derive-per-build discipline, but its own
registry (`InheritedBehavior::dependents` plus the reverse index in `InheritedDependencies`).
Unifying the two registries is #1254's question.

- `FieldMask<D>` — a 64-bit field set typed by the provider's data type — and the opt-in
  `#[derive(InheritedData)]` (one `FIELD_<NAME>: FieldMask<Self>` constant per field plus
  `field_mask_diff`). `depend_on_field::<T, _>` takes `FieldMask<T::Data>`, so a selector of
  another data type is a compile error (a `compile_fail` doctest pins it). Storage and the
  object-safe context method carry the untyped `FieldSet`; the lowering (`FieldMask::erase`) is
  crate-private, so outside `flui-view` a `FieldSet` is only `NONE`/`ALL` (a second `compile_fail`
  doctest). The marker is invariant in `D`, and `BuildContext` is sealed so a downstream context
  cannot forward an erased set to a different provider. Compose and SwiftUI track per property
  with no untyped selector; a typed mask is the closest static shape.
- `InheritedView::changed_fields(old)` — `ALL`/`NONE` from `update_should_notify` by default,
  per-field for data implementing `InheritedData`.
- `InheritedBehavior::dependents` stores `DependentEntry { depth, mask, lifecycle_mask }`;
  `on_view_updated` schedules a dependent only if `mask | lifecycle_mask` intersects the changed
  set. `depend_on::<T, _>` records `FieldSet::ALL` through the same path.
- Field accessors: `MediaQuery::size_of(cx)`, `text_scale_factor_of`, …, `Theme::color_scheme_of`,
  `text_theme_of`, and the general `depend_on_fields(cx, mask, f)`; `MediaQuery::of`/`Theme::of`
  keep the whole-provider dependency.

**Mapping decision — reset-on-build (deliberate divergence from Flutter).** Flutter's
`_dependencies`/`_dependents` accumulate until unmount: a widget that read `MediaQuery.sizeOf`
once keeps rebuilding on size changes after it stopped reading it. Here the fields recorded for
an element are those of its **latest** build: in `BuildOwner::drain_build_scope`, right after the
element's `build`, its masks at previous providers reset to `NONE`, the build's reads
re-accumulate, and an entry still at `NONE` is removed. The `LayoutBuilder`-scoped drain uses the
same function. Pinned by `a_rebuild_re_derives_the_field_set_so_a_dropped_read_stops_depending`
(`media_query_fields.rs`). Cost: one hash lookup per previous provider per build.

- Reads in `init_state` / `did_change_dependencies` go to `lifecycle_mask`, which **accumulates
  until unmount**, as in Flutter. It is not re-derived per `did_change_dependencies` (that hook
  does not run on the first build after `init_state`, so resetting would drop `init_state`'s
  reads). States such as `FocusState` and `DraggableState` acquire an inherited value in a
  lifecycle hook and must stay subscribed
  (`a_dependency_acquired_in_a_lifecycle_hook_survives_a_rebuild_that_does_not_reread_it`).
- A build that panics is not evidence of what the element reads: when `build_or_recover`
  recovers it with an `ErrorView`, previous masks are kept and new records only added; the signal
  registry likewise restores the previous read set
  (`a_build_that_panics_before_reading_keeps_its_dependency`,
  `a_build_that_unwinds_keeps_its_previous_read_set`).

#1090's acceptance tests (`crates/flui-widgets/tests/media_query_fields.rs`,
`packages/flui-material/tests/theme_fields.rs`) pin: a size-only change rebuilds size readers and
whole-`of` readers, not text-scale readers, and the reverse; an equal provider swap rebuilds
nobody; a field reader still rebuilds on its own field after an unrelated change.

### 5.6 What the three screens look like

**Counter** — the handle is `Copy`; the write happens in the tap callback, outside `build`:

```rust
struct CounterState { count: Option<Signal<u32>> }

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.count = Some(cx.signal(0u32));                  // owned by this element
    }
    fn build(&self, _v: &Counter, cx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.expect("BUG: init_state ran before build");
        let r = cx.reactive();
        Column::new((
            Text::new(format!("{}", count.get(cx))),         // this element is a reader
            Button::new("+1").on_tap(move || { let _ = count.update(&r, |c| *c += 1); }),
        ))
    }
}
```

**Todo list, 10 000 rows** — the list reads only the row count; each row reads its own signal,
so toggling one row rebuilds that row (2 elements, §8.1).

**Form, 20 fields** — one signal per field; each `TextField` reads its own. The Save cell reads
all twenty and rebuilds on every keystroke until ADR-0075's `Computed` lands.

A headless test builds the model with the binding's `reactive()`, writes with
`fields[3].set(&r, …)`, pumps a frame, and asserts on the readers rebuilt
(`BuildOwner::last_frame_build_report`), not on pixels.

### 5.7 Rules without hooks

- A signal is created explicitly (`cx.signal(v)` in `init_state`, or `r.signal(v)` at the app
  root) and held in a field. There is no `use_signal()` whose identity is its call index; `build`
  may read signals in any order, conditionally, or in loops.
- `build` stays a pure function of `(View, State, reads)`; reads are recorded, not ordered.
- Lifetime is ownership: an element-owned slot is released at unmount (with that element's
  reads); an app-level slot with the realm. A stale handle is `SignalError::Released` from
  `try_*`/`peek` and a documented panic from `get`/`with`.
- Equality is opt-in: plain `set` marks readers even for an equal value; only `set_if_changed`
  compares.

### 5.8 Threads

- `Signal` and `Reactive` are `!Send + !Sync`, realm-affine like the element tree (ADR-0027).
- Cross-thread writes go through the realm proxy: `UiCommand::SignalWrite(Box<dyn FnOnce(&Reactive)
  + Send>)` runs on the owner thread at the next idle drain, marks readers, and wakes a frame. The
  closure captures `signal.detach()` — a `Send + Sync` `SignalSender<T>` re-attached with
  `.attach()` on the owner side.
- Writes from a dead realm's sender return `OwnerGone`, like every realm-scoped command.
- `UiCommand::SignalWrite` and `UiCommandSender::send_signal_write` are `pub(crate)` until an
  async task API vends a `SignalSender` through the realm handle; this ADR adds no public realm
  API.

### 5.9 Testability

`HeadlessBinding::reactive()` exposes the graph; a test creates signals, mounts a tree that reads
them, writes, pumps a frame, and asserts the value seen and **which elements rebuilt**
(`RebuildReason::SignalChange` in the build report).

### 5.10 `setState`, `ValueNotifier`, `InheritedView` — what happens to them

Nothing is removed.

- `set_state_scheduled` stays for widget-local, short-lived state read by one element; shared,
  long-lived state read by a handful of elements is a signal.
- **Choosing by reader count.** A signal write costs about 1.3–1.8 µs per reader on top of one
  `setState` on the readers' common parent (§8.1, 600 readers), because each reader is scheduled
  individually. **A value with fewer than ~50 readers is a `Signal`; a value read by hundreds of
  cells (theme slot, unit preference, media-query field) is an `InheritedView` with field masks**
  — one dependency edge per subtree, same granularity.
- `ValueNotifier`/`ChangeNotifier` stay the `Listenable` contract for controllers. Bridging is a
  listener that writes the signal, or a listener that `peek`s it; no adapter type ships.
- `InheritedView` stays the scoping mechanism; a provider whose value is a `Signal<T>` is the
  recommended way to put realm state into a subtree.

## 6. Consequences

**Positive.** Application state has a framework-visible home; rebuild scope follows reads, not
widget slicing; `Copy` handles remove listener/handle boilerplate; the headless driver tests state
directly; cross-thread writes reuse the realm command inbox.

**Negative.**
- Reader-set clear-and-rebuild per build is a real (allocation-free) cost per read; idle cost is
  measured in §8.1.
- Two ways to hold state (`setState` and signals); §5.10's guidance belongs in the
  widget-authoring docs.
- A signal read in `build` and written in the same element's `did_update_view` loops until the
  mid-drain absorb budget stops it; the run-time refusal covers `build` only.
- Until ADR-0075 lands, a cell needing a derived value reads every input and pays that rebuild.

## 7. Amendment to FOUNDATIONS C1 (applied)

C1's signals clause is replaced with: *"A realm-owned reactive graph (`Signal` and its reader
registry, ADR-0074; derived values and effects in ADR-0075) is a first-class state layer of the
view crate. Reading a signal in `build` is the sanctioned subscription path (the same class as
`depend_on`); writing or creating a signal inside `build`/`layout`/`paint` is refused at run time.
The catalog crates may accept `Signal<T>` values as widget inputs but never own application
state."* "The smallest sound invalidation unit stays the Element" is unchanged and is what §5.3
relies on. ADR-0008's objection that signals route invalidation around the retained tree is
answered by §5.3: they feed the same heap.

## 8. Measurement

Prototype scope: `Signal`/`Reactive` in `flui-view` behind `signals`, the reader registry,
`RebuildReason::SignalChange`, `HeadlessBinding::reactive()`, one `UiCommand::SignalWrite`. No
catalog changes. The benchmark (`crates/flui-widgets/benches/signals_rebuilds.rs`,
`cargo bench -p flui-widgets --bench signals_rebuilds`; the prototype ran it with
`--features signals`, which no longer exists) runs each scenario as **A** `setState` on the owning state vs **B**
signals, on the same tree.

Acceptance criteria: for "one thing changed" scenarios B rebuilds only the readers (A rebuilds
the owning subtree), relayout count ≤ A, frame time ≤ A, and idle overhead ≤ 5 % of A; no scenario
may need a `PartialEq` bound on plain `Signal<T>`.

### 8.1 Results (2026-09-22)

Dev profile, one run; counts from `BuildOwner::last_frame_build_report` and the frame's
`PipelineOwner::layout_roots_total` delta. Each cell is two elements (a stateless view over a
`SizedBox`). Counts are profile-independent; timings compare A against B only.

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

What the numbers say:

- One changed value rebuilds exactly its readers (plus each reader's child): 2 elements instead
  of 20 003 for a row, 4 instead of 44 for a keystroke; 3.6× and 2.1× faster.
- Relayout counts are equal everywhere; idle cost is unchanged with 10 000 registered readers.
- **Fan-out writes are slower**: 600 readers take 1.77 ms vs 1.01 ms, because each reader is
  scheduled through the inbox individually (#1249). Hence §5.10's rule.
- **Append is no better**: the list root legitimately rebuilds and re-emits every child. Stopping
  propagation at unchanged children is `Memo<V>`/`can_update`'s job, orthogonal to this ADR.

ADR-0075's baseline: the prototype's derived-value scenario (a `Computed` invalid count read by
the Save cell) measured 4 elements / 17.9 µs against 44 / 39.2 µs.

## References

- FOUNDATIONS C1, C5, C8; ADR-0018; ADR-0027 (`UiRealm`, `UiCommandSender`);
  ADR-0078 (capability and build-phase rules); issue #1090.
- `docs/research/state-model-2026.md` — market survey with sources.
- ADR-0075 — derived values and effects; follow-ups #1248–#1254.
