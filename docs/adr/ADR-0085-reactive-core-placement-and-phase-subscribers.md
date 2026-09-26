# ADR-0085: The reactive graph is realm-owned, read through `ReadScope`, and extracted only with a second consumer

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0074](ADR-0074-realm-scoped-signals.md) — §5.1 ("`Reactive` … lives beside
  `BuildOwner`", "reachable as `cx.reactive()`"), §5.2 (reads take `&dyn BuildContext`), and the
  `signals` feature named in its Status line
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

The cross-thread write path picks one of them without looking at the slot.
`UiCommand::SignalWrite` (`crates/flui-app/src/app/ui_realm/commands.rs:449-455`) takes the
graph from `self.widgets()`, which is `self.presentations.primary().widgets()`
(`crates/flui-app/src/app/ui_realm/presentations.rs:353-358`); that accessor is also compiled
out on Android, iOS and wasm (`presentations.rs:353-356`). The slot knows which graph minted it
(`SignalSlot::graph`, `mod.rs:78-82`), and the graph already refuses a foreign slot
(`SignalError::ForeignGraph`, `mod.rs:267-273`).

**Failure scenario.** A view in a secondary window creates a signal in `init_state`; its slot
carries window B's graph id. A worker detaches it (`SignalSender`, `mod.rs:810`), and the write
arrives as `UiCommand::SignalWrite`. The command re-attaches the sender against the primary
window's graph, `set` returns `ForeignGraph`, and window B's readers never rebuild. This is a
conformance defect against ADR-0074. It is latent today: `send_signal_write` has no production
caller (`commands.rs:262-267` carries `expect(dead_code, reason = "cross-thread signal write
sender is wired before public runtime vending")`), and no test opens two presentations and
writes through the command.

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

### Why the crate question is not "now or never"

A `flui-reactive` crate below `flui-view` is the end state the review proposed (D4). Created
today it would have one consumer (`flui-view`), which is the "unwired surface" defect AGENTS.md
names and the compile-seam rule ADR-0081 records. Folding the graph into `flui-foundation`
instead would put element lifecycle vocabulary in the value layer and widen the rebuild set of a
graph edit: `cargo tree -i` counts 27 crates above `flui-foundation` against 16 for a crate
beside `flui-view`/`flui-rendering`/`flui-animation` (counts from the panel record, including the
crate itself; wall-clock cost is unmeasured).

## Decision

### 1. Graphs are realm-owned; writes are routed by the slot

Each presentation keeps its own graph, as today (one `BuildOwner` and one `PipelineOwner` per
presentation, ADR-0043), and the realm owns every presentation, so every graph is realm-owned and
dropped with its realm. A write is routed by the slot, never by "the primary presentation": the
command applies to the graph in this realm whose id equals `SignalSlot::graph`, and a slot whose
graph is not in this realm is `ForeignGraph`. Merging the per-presentation graphs into one graph
per realm is not decided here; it would need its own scheduled step and a reason a cross-window
read needs it.

This half ships first and on its own, before any other step here: a failing test that writes
from a secondary presentation and asserts that the reader in that presentation rebuilds, then
the routing fix.

### 2. Reads go through a read-only `ReadScope`

- `Signal::get`, `with`, `try_get` and `try_with` take `&dyn ReadScope`. `BuildContext:
  ReadScope`, so existing call sites (`count.get(cx)`) compile unchanged through trait upcasting
  (stable since Rust 1.86; the workspace pins 1.98.1).
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

### 4. Readers are phase-typed

The reader set generalises from `ElementId` to

```rust
enum Reader { Element(ElementId), Layout(RenderId), Paint(RenderId) }
```

A write marks `Element` readers for rebuild (as today), `Layout` readers `needs_layout` and
`Paint` readers `needs_paint`, through the render object's own invalidation handle (ADR-0013).
A write during layout or paint is refused or deferred by a phase guard, on the model of
`WrittenDuringBuild`; each guard has a test.

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

### 6. The crate appears with its second consumer

The ordering is three steps, each shippable alone:

1. **Seam inside `flui-view`.** §2, §3 and §5. The reactive module has no `crate::` import
   outside itself, pinned by the module-dependency gate of
   [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md).
2. **Phase readers inside `flui-view`.** §4, with the phase guards.
3. **Extraction.** One change moves the module into `flui-reactive` (tier V, kind internal,
   `publish = false` until a publishing decision) *and* lands the first render-phase subscriber:
   a render-object field read in `paint` through a `ReadScope` implemented by the paint context,
   written outside the frame phases, with a test that fails without the change and observes the
   repaint itself, not the existence of a subscription. The `Listenable` adapter over the graph
   lives in `flui-reactive`.

`ScrollPosition` is not the first subscriber: the viewport writes it from `perform_layout`
(`crates/flui-objects/src/sliver/viewport.rs:1057`, `:1104`, `:1827-1828`) and there is no
policy for writes during layout yet. `CustomPainter` is not either: the trait is `Send + Sync`
(`crates/flui-rendering/src/delegates/custom_painter.rs:105`) and cannot hold a `!Send`
`Signal<T>` without an API change of its own.

If no render-phase subscriber exists by the B1 milestone, the graph stays in `flui-view` and D4
is amended explicitly rather than extracted without a consumer.

**Folding into `flui-foundation` instead** is decided by one measurement taken in the
extraction change: touch the reactive module, time `cargo check -p flui-app`, and compare with
the same edit placed in `flui-foundation`. A single-unit `--timings` figure does not count.

## Alternatives considered

- **Create `flui-reactive` now.** Rejected: one consumer, five driver hooks with no second
  caller, and a new publish unit that proves nothing. It is where this ADR ends, not where it
  starts.
- **Put the graph in `flui-foundation`.** Rejected for now: element-lifecycle vocabulary in the
  value layer, and a graph edit rebuilds every crate above foundation (27 against 16 by
  `cargo tree -i`; the time cost is the measurement §6 requires).
- **Keep the graph in `flui-view` and let render objects subscribe through an erased trait.**
  Rejected: an inherent `Signal<T>` can only be named in its defining crate, so render and
  animation code could never take a `Signal<T>`, and two observation systems (signals and
  `Listenable`) would stay permanent.
- **One `ReactiveDriver` for both phases.** Rejected: the render phase has no `BuildOwner`;
  handing it the element driver would let a pipeline pass begin element builds.
- **`ReadScope` returning `Reactive`.** Rejected: it re-opens the write path from `build` that
  ADR-0086 closes.
- **Keep `signals` as a feature until extraction.** Rejected: the supertrait cannot be gated,
  and a gated read parameter would mean two `Signal::get` signatures.

## Consequences

- ADR-0074's placement sentence, its `cx.reactive()` read path and its feature gate are replaced
  by this record; its semantics, guard and measurement stand. `docs/FOUNDATIONS.md` C1 (line 91)
  loses "`flui-view` feature `signals`" and says "the realm-owned graph" in the same change as §5;
  its sentence "The catalog crates … never take a dependency on a signals crate" is amended in the
  extraction step (§6, step 3) to "never own application state in a signal", because the catalog
  then depends on `flui-reactive` through `flui-view`.
- **Breaks.** `Signal::get(cx)` call sites keep compiling. Code that names `&dyn BuildContext`
  in its own signal helpers changes to `&dyn ReadScope`. Every `cfg(feature = "signals")` and the
  facade's `signals` feature disappear; downstream manifests that enable `flui/signals` must drop
  it. `BuildContext::reactive()` disappears in ADR-0086's change, not here.
- `crates/flui-view/Cargo.toml:118-123`, `Cargo.toml:642-645` and the "no signals crate" note in
  the root member list (`Cargo.toml:72-77`) are rewritten in the change that performs each step.
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

None of these exist yet.

- A headless test with two presentations in one realm: a signal created by a view in the second
  presentation, written through `UiCommand::SignalWrite`, rebuilds that view. It fails on the
  current routing (§1).
- `compile_fail` doctests: `ReadScope` exposes no write or create method; neither driver is
  `Clone`; a `ReadScope` cannot reach a driver hook.
- A module-dependency check that the reactive module imports nothing else from `flui-view`
  (ADR-0081's gate), green after step 1.
- One test per phase guard: a write during layout and a write during paint are refused or
  deferred, as §4 specifies.
- The first render-phase subscriber's test: a signal write outside the frame repaints the render
  object and nothing else, observed through the pipeline's paint record, and fails with the
  subscription removed.
- `cargo tree -p flui-view -e features` shows no `signals` feature after §5; `cargo xtask
  workspace` passes with `flui-reactive` declared at tier V after step 3.
- The warm-edit measurement of §6, recorded in the extraction change.
