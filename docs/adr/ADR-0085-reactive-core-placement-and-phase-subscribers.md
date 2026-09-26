# ADR-0085: The reactive graph is realm-owned and stays in `flui-view`; reads go through a `ReadScope` contract in `flui-foundation`

- **Status:** Proposed
- **Date:** 2026-09-25
- **Revised:** 2026-09-26 (prototype of the read contract; see Context)
- **Amends (on acceptance):** [ADR-0074](ADR-0074-realm-scoped-signals.md) — §5.1 ("`Reactive` … lives beside
  `BuildOwner`", "reachable as `cx.reactive()`"), §5.2 (reads take `&dyn BuildContext`; now any
  `&S` where `S: ReadScope`), and the `signals` feature named in its Status line
- **Related:** [ADR-0013](ADR-0013-render-object-attach-self-dirty-handle.md) (the self-dirty
  handle a render-phase subscriber uses), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (realm
  ownership), [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md)
  (one `BuildOwner` per presentation), [ADR-0075](ADR-0075-derived-state-and-effects.md)
  (derived values and effects on the same graph),
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) (the run-time guard is authoritative),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tier and kind of a new crate),
  [ADR-0086](ADR-0086-signal-writes-through-event-context.md) (the write side)
- **Refs:** decision D4 and owner decision 5 in [`design/decisions.md`](../../design/decisions.md);
  the panel record in
  [`report-decisions.ru.md` §5](../research/2026-09-25-architecture-review/report-decisions.ru.md)

ADR-0074 decided *what* a signal is. This record decides *where the graph lives*, *who owns an
instance of it*, and *what may read it*. It keeps ADR-0074's semantics (read-is-subscribe in
`build`, writes dirty exactly the readers, the run-time guard) and changes the placement, the
read parameter and the feature gate. The write side is ADR-0086.

## Context

### Where the graph is today

The whole graph is one file, `crates/flui-view/src/reactive/mod.rs` (1159 lines). Its outside
dependencies are small: `flui_foundation::{ElementId, RebuildReason}` (`mod.rs:65`), `smallvec`,
and two things from its own crate — `crate::owner::ExternalBuildScheduler` (`mod.rs:68`) and
`crate::BuildContext`, which `Signal::get` takes as its read parameter
(`mod.rs:752`: `pub fn get(self, cx: &dyn crate::BuildContext) -> T`). Because `get` is an
inherent method, `Signal<T>` and the graph must live in the same crate as whatever names them;
`flui-rendering` and `flui-animation` sit below `flui-view` and cannot name a `Signal<T>` at all.

The graph only knows elements as readers. Every reader slot is an `ElementId`:
`Node::element_readers` (`mod.rs:137`), `Inner::element_reads` (`mod.rs:145`),
`Inner::building` (`mod.rs:150`), `SlotInfo::readers` (`mod.rs:191`) and
`Reactive::readers_of` (`mod.rs:469`). A render object that paints from a signal therefore has no
way to subscribe; it has to be rebuilt through an element.

The graph is reachable from every context. `BuildContext::reactive()`
(`crates/flui-view/src/context/build_context.rs:131-132`) returns the owning `Reactive` handle,
which is `Clone` (`mod.rs:160-164`) and exposes `set`/`update` (`mod.rs:774`, `:783`). A `build`
that calls `cx.reactive()` and writes is refused only at run time (`WrittenDuringBuild`,
`mod.rs:578`).

### The graph is per presentation, not per realm

ADR-0074 §4 says "a realm-owned reactive graph" and §5.1 says it "lives beside `BuildOwner`,
dropped with the realm". In the code the graph is a field of `BuildOwner`
(`crates/flui-view/src/owner/build_owner.rs:443-444`), constructed with it (`:721-722`) and
exposed by `pub fn reactive` (`:972-974`). ADR-0043 gives every presentation its own
`BuildOwner`, so a realm with two windows has two graphs, each with its own process-unique id
(`static NEXT_GRAPH_ID`, `mod.rs:72`).

Until §1 shipped, the cross-thread write path picked one of them without looking at the slot:
`UiCommand::SignalWrite` took the graph from `self.widgets()`, which is
`self.presentations.primary().widgets()`, an accessor that is also compiled out on Android, iOS
and wasm. The slot knows which graph minted it (`SignalSlot::graph`), and the graph already
refuses a foreign slot (`SignalError::ForeignGraph`).

**Failure scenario (before §1).** A view in a secondary window creates a signal in
`init_state`; its slot carries window B's graph id. A worker detaches it (`SignalSender`), and
the write arrives as `UiCommand::SignalWrite`. The command re-attached the sender against the
primary window's graph, `set` returned `ForeignGraph`, and window B's readers never rebuilt.
This was a conformance defect against ADR-0074, fixed by §1. It was latent: `send_signal_write`
has no production caller (it carries `expect(dead_code, reason = "cross-thread signal write
sender is wired before public runtime vending")`), and no test opened two presentations and
wrote through the command.

### The feature gate

Signals are off by default. `crates/flui-view/Cargo.toml:118-123` says "Off by default until
the go/no-go measurement in the ADR lands"; the facade's comment (`Cargo.toml:642-645`) says it
"stays an opt-in until the #1090 field-mask registry lands on the same seam".

- The measurement is recorded in ADR-0074 §8.1. It meets the rebuild-count and idle-overhead
  criteria and misses "frame time ≤ A" in two scenarios: fan-out (600 readers, 1.77 ms against
  1.01 ms) and append (35.57 ms against 33.98 ms). ADR-0074 already accepted both, with §5.10's
  fan-out rule and `Memo<V>` for append.
- #1090 has landed (`588251a1c`): `FieldMask` (`crates/flui-view/src/view/inherited.rs:114`),
  `depend_on_field` (`crates/flui-view/src/context/build_context.rs:648`) and the acceptance
  tests `crates/flui-widgets/tests/media_query_fields.rs` and
  `crates/flui-material/tests/theme_fields.rs`.

Both manifest comments therefore name a precondition that is already met or already decided.
What remains open is ADR-0074 §5.5's second registry: field masks and signal readers are two
registries on one scheduler, not one.

### Why the crate question is settled by measurement

- The review proposed a `flui-reactive` crate below `flui-view` (D4) so that `flui-rendering` and
  `flui-animation` could name `Signal<T>`, which the inherent `get(self, cx: &dyn
  crate::BuildContext)` (`mod.rs:752`) confines to `flui-view`.
- The earlier text of this record estimated the rebuild set of a graph edit at 27 crates in
  `flui-foundation` against 16 in a crate beside `flui-view`/`flui-rendering`/`flui-animation`,
  by `cargo tree -i`, with wall-clock time unmeasured.
- It therefore made the placement depend on a warm-edit measurement (§6 of that text). The
  prototype below took it.

### What a prototype showed (2026-09-26)

Branch `spike/readscope`, commits `28ed25536` (variant A, reads take `&dyn ReadScope`) and
`ae8c1afa5` (variant B, reads take a generic `&S`). Not merged.

- **Size and reach.** 19 files, +821/−373. `flui_foundation::read_scope` is 427 lines with no
  new dependency. `cargo check -p flui-view -p flui-rendering` is green. `git diff --stat` over
  `flui-widgets`, `flui-app`, `flui-testing`, `src` and `examples` is empty. A reviewer re-ran
  `cargo check -p flui --features signals --all-targets` and `cargo xtask workspace`, both green.
- **Warm edit** (`cargo check -p flui-app`, one run on a shared host). An item added to
  `read_scope.rs` re-checks **15 crates in 5.74 s**. The same edit in `reactive/mod.rs`
  re-checks **3 crates (view, widgets, app) in 3.07 s**. The crate counts are cargo's re-check
  set; the times only corroborate them.
- **Benchmark.** `signals_rebuilds` B/A ratios stay within main's own run-to-run spread. The
  unchanged control moved by up to 60% between runs, so nothing below that resolution is
  claimed. Every run had `signals` on, so default builds are unmeasured.
- **Variant A against variant B.** `&dyn ReadScope` rejects `&&dyn BuildContext` and
  `&Box<dyn BuildContext>`, which compile today by deref coercion; blanket impls fix both. It
  also rejects `fn f<C: BuildContext + ?Sized>(cx: &C) { sig.get(cx) }` (E0277). That shape does
  not compile on main either (the parameter is `&dyn BuildContext`), so it is not a regression.
  Variant B accepts all six shapes probed, including `&dyn ReadScope`.
- **Holes the prototype opened** (found in review; the Decision closes each):
  - `Signal::from_slot` and `SignalSlot::new` became public, so
    `Signal::<String>::from_slot(u32_sig.slot()).get(cx)` panics with a `BUG:` message, which
    breaks [`docs/PANIC-POLICY.md`](../PANIC-POLICY.md).
  - `ReadGraph::register_reader` and `ScopeRef::new(graph, Some(reader))` let any holder of a
    `Reactive` (through `cx.reactive()`) subscribe an arbitrary `ElementId` or `RenderId`.
  - `PaintCx` became `!Send`/`!Sync`.
  - The render reader is recorded but not marked on write.
  - `PaintCx::with_read_scope` has no production caller.
  - The trybuild snapshot `tests/ui/build_context_is_sealed.stderr` lists every `ReadScope`
    implementor.
  - The new tests go through `ElementBuildContext`, which production never constructs
    (production uses `BuildCtx`).

## Decision

### 1. Graphs are realm-owned; writes are routed by the slot

Each presentation keeps its own graph, as today (one `BuildOwner` and one `PipelineOwner` per
presentation, ADR-0043), and the realm owns every presentation, so every graph is realm-owned and
dropped with its realm. A write is routed by the slot, never by "the primary presentation": the
command applies to the graph in this realm whose id equals `SignalSlot::graph`. A slot whose
graph no presentation of this realm owns (its presentation closed, or it was minted by another
realm; process-unique graph ids cannot tell the two apart) is dropped without running the
write, counted as stale by the drain and logged as a `warn` on `flui::signals`; the writer is
not told until ADR-0086 gives writes a return path. Merging the per-presentation graphs into
one graph per realm is not decided here; it would need its own scheduled step and a reason a
cross-window read needs it.

The command carries its routing key: `UiCommand::SignalWrite { target: SignalSlot, apply }`,
built by `send_signal_write(target: SignalSender<T>, apply: impl FnOnce(Signal<T>, &Reactive))`,
which takes the slot from the handle so a write cannot be addressed to one graph and performed
against another. The drain requests the owning presentation's frame itself, because a
secondary presentation's `BuildOwner` has no wake hook of its own.

This half ships first and on its own, before any other step here: a failing test that writes
from a secondary presentation and asserts that the reader in that presentation rebuilds, then
the routing fix.

### 2. Reads go through a read contract in `flui-foundation`

The read vocabulary lives in `flui_foundation::read_scope`; the graph that implements it stays
in `flui-view` (§6). The module's public items are normative:

```rust
pub struct SignalSlot { /* graph, index, generation: private */ }
impl SignalSlot {
    #[doc(hidden)] pub const fn new(graph: u32, index: u32, generation: u32) -> Self; // for the graph only
    pub const fn graph(self) -> u32;
    pub const fn index(self) -> u32;
    pub const fn generation(self) -> u32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignalError {
    Released { index: u32, generation: u32 },
    ForeignGraph { index: u32, graph: u32, this: u32 },
    TypeMismatch { index: u32, expected: &'static str }, // replaces the `BUG:` panic
    WrittenDuringBuild { element: ElementId },
    CreatedDuringBuild { element: ElementId },
    Reentrant { index: u32 },
    NoGraph,
} // `Display` and `Error` by hand: foundation takes no `thiserror`

/// Pure reads: no subscribe, write, create or release.
pub trait ReadGraph {
    fn graph_id(&self) -> u32;
    fn read_erased(&self, slot: SignalSlot, read: &mut dyn FnMut(&dyn Any)) -> Result<(), SignalError>;
}

/// Bound to one reader by whoever minted it.
pub trait ReaderSink {
    fn subscribe(&self, slot: SignalSlot);
}

#[derive(Clone, Copy)]
pub struct ScopeRef<'a> { /* graph: Option<&'a dyn ReadGraph>, sink: Option<&'a dyn ReaderSink>; no accessors */ }
impl<'a> ScopeRef<'a> {
    pub fn new(graph: &'a dyn ReadGraph, sink: Option<&'a dyn ReaderSink>) -> Self;
    pub const fn detached() -> Self;
}

pub trait ReadScope { fn scope(&self) -> ScopeRef<'_>; }
impl<S: ReadScope + ?Sized> ReadScope for &S { /* .. */ }
impl<S: ReadScope + ?Sized> ReadScope for Box<S> { /* .. */ }

pub struct Signal<T: 'static>; // Copy, !Send
impl<T: 'static> Signal<T> {
    #[doc(hidden)] pub const fn from_slot(slot: SignalSlot) -> Self;
    pub const fn slot(self) -> SignalSlot;
    pub const fn detach(self) -> SignalSender<T>;
    pub fn try_with<S: ReadScope + ?Sized, R>(self, cx: &S, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
    pub fn try_get<S: ReadScope + ?Sized>(self, cx: &S) -> Result<T, SignalError> where T: Clone;
    pub fn with<S: ReadScope + ?Sized, R>(self, cx: &S, f: impl FnOnce(&T) -> R) -> R; // `# Panics` documented
    pub fn get<S: ReadScope + ?Sized>(self, cx: &S) -> T where T: Clone;                // `# Panics` documented
    pub fn peek<R>(self, graph: &dyn ReadGraph, f: impl FnOnce(&T) -> R) -> Result<R, SignalError>;
}

pub struct SignalSender<T: 'static>; // Send + Sync; attach() -> Signal<T>; slot() -> SignalSlot
```

- **The read parameter is `&S where S: ReadScope + ?Sized`.** It accepts every context shape
  that compiles today, plus a generic `?Sized` context and `&dyn ReadScope`. It is chosen because
  it accepts strictly more shapes than `&dyn ReadScope`, not because it fixes a break: the
  blanket impls already repair what `&dyn ReadScope` would reject (Context).
- **`BuildContext: ReadScope` stays a supertrait**, so `count.get(cx)` keeps its spelling.
- **Subscription is not a method of the graph.** `Reactive` implements `ReadGraph`, which is
  enough for `peek`, and never `ReaderSink`. Sinks are private types in `flui-view`, minted by
  the drivers of §3: `ElementDriver` for `BuildCtx` during `build`, `RenderDriver` for layout
  and paint. A `ScopeRef` built outside `flui-view` can subscribe only through a sink its
  builder was given, so a forged scope reads and cannot subscribe anyone on a real graph.
- **A handle of the wrong `T` is a typed error.** `try_with`/`try_get` return
  `SignalError::TypeMismatch`; `with`/`get` panic with a documented `# Panics` message, not a
  `BUG:` one. The two constructors are `#[doc(hidden)]` because `flui-view` needs them across the
  crate boundary. The graph's "a live slot holds a value of another type" `expect`s go away.
- **Writes are an extension trait**, `pub trait SignalWriteExt<T: 'static>: Copy` in
  `flui_view::prelude`, with `set(self, r: &Reactive, value: T)`, `update` and `set_if_changed`,
  the signatures of today (`mod.rs:774`, `:783`, `:793`). It lasts until ADR-0086 replaces
  `&Reactive`.
- `ReadScope` can register a read against the current subscriber and identify its graph
  (`fn scope(&self) -> ScopeRef<'_>`). It cannot return a `Reactive`, cannot write, and cannot
  create a slot.
- `BuildContext::reactive()` goes away together with the write-signature change in ADR-0086; it
  is the last path from `build` to a writable graph handle.

### 3. Two non-`Clone` drivers, one per phase family

For each presentation, the realm mints exactly two drivers from that presentation's graph:

- `ElementDriver`, held by `BuildOwner`: begin/end element build, register an element reader,
  release an element's slots, set the rebuild sink.
- `RenderDriver`, handed to `PipelineOwner`: register a render object as a layout or paint reader
  and bracket the layout and paint passes.

Neither is `Clone`, and neither is reachable from `ReadScope`, `BuildContext` or
`LifecycleContext`. `ExternalBuildScheduler` is replaced by a one-method `RebuildSink`, so the
graph no longer imports anything from `flui-view`'s owner module.

`PipelineOwner` reaches its `RenderDriver` through dependency inversion: a trait object whose
trait `flui-rendering` declares and `flui-view` implements, so `flui-rendering` never names the
graph. The driver mints the layout and paint `ScopeRef`s, and `PaintCx` exposes the one it is
given through `PaintCx::with_read_scope(self, scope: ScopeRef<'a>) -> Self`, not a
`(graph, node)` pair as in the prototype. The drivers are also what mint the `ReaderSink`s of
§2.

### 4. Readers are phase-typed

The reader set generalises from `ElementId` to

```rust
enum Reader { Element(ElementId), Layout(RenderId), Paint(RenderId) }
```

`Reader` is a type of `flui-view`, not of `flui-foundation`: a sink carries its reader, so the
read contract never names one.

A write marks `Element` readers for rebuild (as today), `Layout` readers `needs_layout` and
`Paint` readers `needs_paint`, through the render object's own invalidation handle (ADR-0013),
which `flui-view` already reaches because it depends on `flui-rendering`. The write side does
this marking, not only the read side's recording: the prototype recorded render readers and
never marked them. A write during layout or paint is refused or deferred by a phase guard, on
the model of `WrittenDuringBuild`; each guard has a test.

### 5. Signals are not a feature

The `signals` feature is removed from `flui-view`, `flui-widgets`, `flui-app`, `flui-testing` and
the facade in the change that introduces `ReadScope` (§2), because a supertrait bound
(`BuildContext: ReadScope`) cannot be gated by `cfg`. The owner decided on 2026-09-25 to remove
it.

The go/no-go preconditions that the manifest comments name are met, with this evidence:

- **The field-mask registry.** #1090 landed as `588251a1c` (PR #1260): `FieldMask`
  (`crates/flui-view/src/view/inherited.rs:114`), `depend_on_field`
  (`crates/flui-view/src/context/build_context.rs:648`), and the acceptance tests
  `crates/flui-widgets/tests/media_query_fields.rs` and
  `crates/flui-material/tests/theme_fields.rs`.
- **The measurement.** ADR-0074 §8.1 records it. The two frame-time misses (fan-out and append)
  are the cases ADR-0074 already bounds with §5.10's fan-out rule and `Memo<V>`.

Accepting this ADR is therefore the go decision on ADR-0074 §8.1. Unifying the field-mask and
signal registries (#1254) is not a precondition.

Removing the feature puts `begin_element_build`/`end_element_build` on every build and
`release_element` on every unmount of a default build. The change that removes it measures that
cost: `signals_rebuilds` and the idle frame with default features, main against the change.

### 6. The graph stays in `flui-view`; there is no `flui-reactive` crate

The ordering is three steps, each shippable alone, all in `flui-view` plus the contract in
`flui-foundation`:

1. **The contract and the seam.** `flui_foundation::read_scope` (§2), `SignalWriteExt`, the two
   drivers and their sinks (§3), `RebuildSink` in place of `ExternalBuildScheduler`, and §5. The
   accessors §1 relies on stay: `SignalSlot::graph`, and `SignalSender::slot` once the §1 routing
   fix has added it.
2. **Phase readers.** §4, with the phase guards and the write-side marking.
3. **The first render-phase subscriber.** A render-object field read in `paint` through
   `PaintCx: ReadScope`, with the scope minted by `RenderDriver`, written outside the frame
   phases, with a test that fails without the change and observes the repaint itself, not the
   existence of a subscription. This is the first production caller of
   `PaintCx::with_read_scope`, which does not merge before this step.

`ScrollPosition` is not the first subscriber: the viewport writes it from `perform_layout`
(`crates/flui-objects/src/sliver/viewport.rs:1057`, `:1104`, `:1827-1828`) and there is no
policy for writes during layout yet. `CustomPainter` is not either: the trait is `Send + Sync`
(`crates/flui-rendering/src/delegates/custom_painter.rs:105`) and cannot hold a `!Send`
`Signal<T>` without an API change of its own.

If no render-phase subscriber exists by the B1 milestone, `Reader::Layout`/`Paint`,
`RenderDriver` and `PaintCx: ReadScope` are removed rather than kept as unwired surface. The
foundation contract stays, because production element reads use it.

**Why the graph is not extracted.**

- **The motivation for extraction is met without it.** The prototype compiled
  `PaintCx: ReadScope` with no manifest change, so render and animation code can name
  `Signal<T>` and read through `ReadScope` while the graph stays in `flui-view`.
- **Extraction would make the frequent edit more expensive.** A graph edit where the graph is
  re-checks 3 crates in 3.07 s. A `flui-reactive` crate would have to sit below
  `flui-rendering` and `flui-animation`, since both would depend on it to name its types, so
  every graph edit would re-check about 7 crates: the new crate plus the union of the reverse
  closures of `flui-rendering` and `flui-animation` (`cargo tree -p flui-app -e normal -i
  <crate>`: rendering, animation, objects, view, widgets, app). That count is inferred from
  those closures, not measured for a `flui-reactive` crate, and no time is claimed for it. The
  graph is the part that changes (scheduling, guards, phase readers); the contract is small and
  changes rarely, so it is the part that carries the 15-crate foundation cost.
- **Folding the graph itself into `flui-foundation` is rejected** for the same reason, and
  because it would put build-scheduling logic in the value tier. The contract adds no
  element-lifecycle type to foundation: `Reader` stays in `flui-view` (§4), and its only
  build-phase vocabulary is two `SignalError` variants carrying the `ElementId` foundation
  already defines.
- **The two observation systems converge without a crate.** The `Listenable` adapter over a
  signal lives in `flui-view` beside the graph and implements foundation's `Listenable`. It
  lands when the callback surface loses `Send` (ADR-0091 §1), because `Listenable: Send + Sync`
  today (`crates/flui-foundation/src/notifier.rs:78`).
- **No module-dependency gate for the reactive module.** Its only purpose was a mechanical
  extraction. `RebuildSink` stays, for decoupling.
- **Reopening this takes a new ADR.** The trigger is a crate below `flui-view` that needs to
  *own or write* signals, not read them.

## Alternatives considered

- **Create `flui-reactive` now.** Rejected: one consumer, five driver hooks with no second
  caller, and a new publish unit that proves nothing.
- **Extract `flui-reactive` together with the first render subscriber** (this record's earlier
  §6 step 3). Rejected: the read contract already lets render code name `Signal<T>`, and the
  extraction would move every graph edit from 3 re-checked crates to about 7 (inferred, §6).
- **Put the graph in `flui-foundation`.** Rejected: a graph edit would re-check the
  foundation-level set (15 crates, 5.74 s, measured for the contract with `cargo check -p
  flui-app`, one run) against 3 crates, 3.07 s in `flui-view`, and build-scheduling logic would sit in the value tier.
- **Keep `Signal<T>` in `flui-view` and let render objects subscribe through an erased trait.**
  Rejected: an inherent `Signal<T>` can only be named in its defining crate, so render and
  animation code could never take a `Signal<T>`. The adopted design keeps only the *graph* in
  `flui-view`; `Signal<T>` moves to the contract.
- **`&dyn ReadScope` as the read parameter.** Rejected: it accepts strictly fewer shapes than a
  generic `&S` (no generic `?Sized` context), while the blanket impls repair the deref shapes
  for both, so the generic form costs nothing it would fix.
- **Reads through a prelude extension trait.** Rejected: the same coercion limits, and it breaks
  call sites that import `Signal` without the prelude. An extension trait carries writes only
  (`SignalWriteExt`).
- **Reader identity inside the scope** (`ScopeRef::new(graph, Some(reader))`,
  `ReadGraph::register_reader`). Rejected: any holder of the graph could subscribe any node.
- **One `ReactiveDriver` for both phases.** Rejected: the render phase has no `BuildOwner`;
  handing it the element driver would let a pipeline pass begin element builds.
- **`ReadScope` returning `Reactive`.** Rejected: it re-opens the write path from `build` that
  ADR-0086 closes.
- **Keep `signals` as a feature until the contract lands.** Rejected: the supertrait cannot be
  gated, and a gated read parameter would mean two `Signal::get` signatures.

## Consequences

- ADR-0074's placement sentence, its `cx.reactive()` read path and its feature gate are replaced
  by this record; its semantics, guard and measurement stand. `docs/FOUNDATIONS.md` C1 (line 91)
  loses "`flui-view` feature `signals`" and says "the realm-owned graph" in the same change as §5;
  its sentence "The catalog crates … never take a dependency on a signals crate" stays true,
  because there is no signals crate (§6).
- **Breaks.** `Signal::get(cx)` call sites keep compiling. Helpers that take `&dyn BuildContext`
  for reads keep compiling and may generalise to `&S where S: ReadScope + ?Sized`. `.set`,
  `.update` and `.set_if_changed` without `flui_view::prelude::*` need
  `use flui_view::SignalWriteExt`; the prototype showed that no in-tree site breaks. Every
  `cfg(feature = "signals")` and the facade's `signals` feature disappear; downstream manifests
  that enable `flui/signals` must drop it. `BuildContext::reactive()` disappears in ADR-0086's
  change, not here.
- `PaintCx` becomes `!Send`/`!Sync` once it holds a scope. Nothing requires `Send` of it today;
  the frame path is owner-thread (ADR-0091).
- The contract enters the facade's Stable closure: `flui` and `flui-view` re-export `Signal`,
  `ReadScope` and `SignalError`, so ADR-0081's closure measure counts them. It is the first
  `flui-foundation` module whose items the facade promises.
- `flui-foundation` stays free of `thiserror`: `SignalError` implements `Display` and `Error` by
  hand.
- `crates/flui-view/Cargo.toml:118-123` and `Cargo.toml:642-645` are rewritten in the change that
  removes the feature. The "no signals crate" note in the root member list (`Cargo.toml:72-77`)
  stays true.
- The `Arc<Mutex<…>>` notifier in `flui-foundation`
  (`crates/flui-foundation/src/notifier_generic.rs:41-45`) stays for `Send + Sync` users until
  the UI callback surface loses `Send` (scheduled by
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §1), and is then removed
  in favour of the adapter.
- `NEXT_GRAPH_ID` (`mod.rs:72`) is used only through `fetch_add` (`mod.rs:204`), so it is a
  monotonic counter that [ADR-0097](ADR-0097-no-process-global-state-gate.md) §2 exempts, not an
  allowlist entry.
- Migration order and milestone mapping live in
  [the migration plan](../plans/2026-09-25-architecture-migration-plan.md).

## Verification

Only the first exists.

- §1: `a_write_to_a_secondary_presentations_signal_rebuilds_its_reader` in
  `crates/flui-app/src/app/ui_realm/tests/signal_write_routing.rs` writes, through
  `UiCommand::SignalWrite`, a signal minted by the second presentation of a realm and asserts
  that its reader rebuilds and the primary's does not. It failed with `ForeignGraph` on the
  primary-only routing. Its siblings pin the dropped-and-counted case for a closed presentation
  and for a foreign graph, and the owning presentation's frame request.

The rest is planned. The prototype on `spike/readscope` showed the items marked (prototype); none is merged.

| Item | What it asserts | Why it fails today |
|---|---|---|
| `signal_reads_accept_every_context_shape` (a compiled `flui-view` test) (prototype) | `sig.get(cx)` compiles for `&dyn BuildContext`, `&&dyn BuildContext`, `&Box<dyn BuildContext>`, a generic `C: BuildContext + ?Sized`, `&dyn ReadScope` and `&dyn LifecycleContext` | `ReadScope` does not exist; the generic `?Sized` shape cannot coerce to `&dyn BuildContext` |
| `a_read_in_build_subscribes_through_the_production_context` (through a `flui-testing` mount, so `BuildCtx`; also run with `--release`) | A write rebuilds exactly the reader | Passes on main; it guards the production path the prototype's `ElementBuildContext` tests missed, and the release run guards the half gated on `debug_assertions` |
| `a_signal_handle_of_the_wrong_type_is_a_typed_error` | `try_get` returns `Err(SignalError::TypeMismatch { .. })` | No such variant; the prototype panics with `BUG:` |
| `compile_fail` pair: `Reactive` is not a `ReaderSink`; a driver-minted sink is | The graph handle cannot subscribe; the compiling twin proves the `compile_fail` fails for the right reason | `ReaderSink` does not exist, so the twin is mandatory |
| `compile_fail`: `ScopeRef` exposes no graph; no driver is `Clone`; `ReadScope` reaches no driver hook | The read side cannot write, create or drive | No drivers, no `ScopeRef` |
| Trybuild `build_context_is_sealed.rs` implements `ReadScope` for `Mine` | The snapshot holds only the seal error, not a list of implementors | Today's snapshot lists implementors |
| One test per phase guard (step 2); the first render subscriber's repaint test (step 3) | A write during layout or paint is refused or deferred as §4 specifies; a write outside the frame repaints the render object and nothing else, observed through the pipeline's paint record, and fails with the subscription removed | No phase readers |
| Default-build `signals_rebuilds` and idle-frame numbers | Recorded in the change for §5 | Unmeasured |
| `cargo tree -p flui-view -e features` | No `signals` feature after §5 | The feature exists |
