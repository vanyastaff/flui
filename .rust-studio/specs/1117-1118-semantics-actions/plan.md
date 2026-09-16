# Surface the modeled semantics contract on `flui_widgets::Semantics` (issues #1117, #1118 step 1)

Repo: `/home/vanyastaff/orca/workspaces/flui/huchen` (Rust, FLUI — a Flutter-inspired declarative UI
framework with a three-tree View → Element → Render architecture). Branch:
`vanyastaff/gh-issues-similar-category`.

Flutter reference verified present and pinned: `git -C .flutter describe --tags` → `3.44.0`.

## Problem (verified, not assumed)

Two GitHub issues share one root cause: **`flui_semantics` already models a rich contract, but the
public widget layer cannot reach it.** Nothing is missing below; the *seam* is missing.

Evidence gathered by reading the code:

- `crates/flui-widgets/src/semantics/mod.rs` — the public `Semantics` widget exposes **30 property
  builders** (`label`, `checked`, `live_region`, `role`, …) and **zero action builders**. Enumerated
  rather than estimated, and the enumeration has to be scoped to the right `impl`: `impl Semantics`
  (HEAD lines 62–303) holds **34** `pub fn`: three constructors (`new`, `from_properties`,
  `from_configuration`), one `child`, and 30 property setters. So 30.
  Two earlier drafts of this plan got this wrong in two different ways, and the ways are worth
  keeping: the first said 33 as a guess; the second said 32 from "36 `pub fn` take `mut self`, minus
  `new`/`from_properties`/`from_configuration`/`child`". Both numbers came from subtracting an
  impl-scoped list from a whole-file count (35 `mut self` across all four widgets in the file: 30 on
  `Semantics`, 1 on `ExcludeSemantics`, and 4 `child`), and the second also subtracted three
  constructors that never took `mut self` and so were never in the set it subtracted from. A
  count is only as good as the scope it was taken over.
- `crates/flui-semantics/src/configuration.rs:705` already has
  `add_action(action, handler: SemanticsActionHandler)`; `action.rs:217` defines
  `SemanticsActionHandler = Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>`.
  There are **24** `SemanticsAction` variants and **6** `ActionArgs` variants.
- `crates/flui-semantics/src/configuration.rs` has `set_in_mutually_exclusive_group`, and
  `accesskit_translation.rs` maps a checkable node *in a mutually-exclusive group* to
  `Role::RadioButton` — but `flui_widgets::Semantics` has no builder for it, so
  `crates/flui-material/src/radio.rs:346` publishes only `.checked().enabled()` and every mounted
  `Radio` resolves to `Role::CheckBox`. That is issue **#1117**.
- Verified claim from #1118: `rg 'add_action\(' crates/` finds **no production** widget/material
  call site. Only `flui-app` probes and `flui-semantics`'s own tests. So no control in the catalog
  publishes an activation action; a screen reader can identify a button but cannot activate it.
- `crates/flui-objects/src/proxy/semantics.rs` — `RenderSemanticsAnnotations` has no action setter;
  actions can only ride inside `SemanticsConfiguration`, and the widget is the only way in.

## The composition defect adversarial review found, and how it is fixed

**This was a real hole in the first version of this change, caught before it shipped.** The tests
mounted `Radio` as the render root, where `is_root` forces the node to form (`semantics.rs:427-431`,
`:496-505`), so nothing could absorb its flags. In the composition users actually write the story is
different, and it was verified by execution rather than by reading:

```
a Radio under an annotated ancestor, probed on the tree:
  Semantics::new().button(true).child(Radio::new(..).on_changed(..))
  → a11y roles = [GenericContainer, Button]      ← expected RadioButton
```

The mechanism: an annotated **non-boundary** ancestor absorbs a descendant's config fragments into
one node, so the merged node carries `IsButton` (from any `Semantics(button: true)`) *alongside*
`HasCheckedState` and `IsInMutuallyExclusiveGroup`. Flags survive absorption by union
(`flags.rs:211-213`), and `resolve_role` is an ordered else-if — so the role came down to
precedence, and `IsButton` was tested long before the checkable branch.

**That merge is conditional, and an earlier draft of this plan named a composition where it does
not happen.** `is_compatible_with` (`configuration.rs`) treats *any* overlapping flag bit as a
conflict, and both `ListTile` (`.enabled(self.enabled)`) and `Radio` (`.enabled(interactive)`) set
`HasEnabledState` — so a `Radio` inside a real `ListTile` never merges. Because they do not merge,
the radio keeps a node of its own, and that node announces as a radio: measured through
`crates/flui-material/tests/list_tile.rs`'s `MediaQuery`-wrapped fixture, the composition exports
`[GenericContainer, Button, RadioButton]`. The reorder is not what produces it — the radio's own node
carries no `IsButton`, so the same roles come back with the cascade reverted. What the reorder *does*
fix is the composition whose ancestor publishes `button(true)` and no state flag — measured,
`Semantics::new().button(true).child(Radio…)` resolves the same node id to `Button` before and
`RadioButton` after. The tile is fixed by `Radio` publishing `.in_mutually_exclusive_group(true)`,
which is why that half ships here too.

An earlier figure in this plan (`[GenericContainer, Button]`, "no `RadioButton` node at all") was an
artifact of the mount, not a measurement of the composition: `ListTile::build` composes `SafeArea`,
which reads `MediaQuery::of` and panics with no ambient `MediaQuery`; the panic is contained, the
test still passes, and the radio below `SafeArea` never mounts.

**The defect predates this change**, in both compositions, but only the second is on the path #1117
claims to fix. The first is recorded in the PR body and in
`crates/flui-semantics/ARCHITECTURE.md`'s mapping decisions rather than fixed or claimed, and no
issue was filed for it.

**The fix is in `resolve_role`, and it is the reference-faithful one.** `HasToggledState` and
`HasCheckedState` are strictly more specific than the broad `IsButton`, so they are tested first: a
control that reports a checked or toggled state is a checkable whatever else it also is. The
reference resolves the same collision, and its corpus asserts one node carrying `isButton` beside
`SemanticsFlag.isInMutuallyExclusiveGroup` (`test/material/radio_list_tile_test.dart`) — but for a
composition FLUI cannot currently reach, because the reference's merge predicate is a *restricted*
conflict set (`hasConflictingFlags`), not a raw bit intersection. That divergence is pre-existing
and recorded rather than fixed; the precedence here is what FLUI needs *given* it, not a
reconciliation of the two.

> **Plan-time claim, withdrawn during review (kept as the record of what was believed).** The
> reference's predicate is *not* narrower: its tristate `hasConflict` is `both != none`, which is the
> same "both carry the trait" test as FLUI's bit intersection. The oracle's one node is
> `RadioListTile` wrapping its tile in `MergeSemantics`; FLUI *can* reach that composition
> (`MergeSemantics::new().child(ListTile…Radio)`), and measured it yields one `RadioButton` node —
> pinned in `crates/flui-material/tests/list_tile.rs`. See `crates/flui-semantics/ARCHITECTURE.md`. `container(true)` was rejected as the fix: it would force the radio into a separate node,
which diverges from the reference's structure (the reference merges here) rather than fixing the
precedence.

Evidence: red first, then green after the reorder, on `Semantics::new().button(true).child(Radio…)`
— `[GenericContainer, Button]` before, `[GenericContainer, RadioButton]` after, same node id. The
reorder was additionally revert-and-measured to confirm the `ListTile` composition is unchanged by
it, which is how the draft's error was found. `crates/flui-semantics` + `flui-widgets` → 2319
passed; `flui-material` + `flui-rendering` + `flui-app` → 2187 passed. Pinned by
`a_radio_nested_under_an_annotated_ancestor_still_announces_as_a_radio_button` (that composition,
not the root-mounted shape) and crate-locally by
`a_checkable_beside_is_button_still_resolves_to_the_checkable`.

## Scope of THIS change

**Closes #1117 completely. Lands #1118's step 1 (its own "Possible Direction" item 1) plus the
testability seam.** #1118's catalog-wiring half (its steps 2–5: wiring Material
buttons/checkables/list tiles/tabs) is **explicitly deferred to a follow-up**.

**Why it is deferred — and it is not the reason it first looked like.** The deferral was originally
justified as "the ownership question is still open: does `InkWell` or the higher-level control own
activation?" **The reference settles that, and it settles it the other way.** Flutter's `InkResponse`
— the primitive `InkWell` extends — publishes `Semantics(onTap: simulateTap)` itself
(`.flutter/packages/flutter/lib/src/material/ink_well.dart:1402`, tag `3.44.0` verified), gated by
`excludeFromSemantics || onTap == null`; `Button`, `Checkbox` and `ListTile` publish **no** tap
semantics of their own and rely on the `InkWell` they build. `simulateTap` calls the very
`handleTap()` the pointer path calls (`:902-905`), so AT activation and pointer activation converge
on one code path rather than a parallel one. The lower-level interactive primitive owns activation;
that question is closed.

What actually blocks the catalog half is **storage and threading**: FLUI's widget callbacks are
`Rc<dyn Fn>` (56 aliases, owner-thread-local, `!Send`), while a `SemanticsActionHandler` is
`Arc<dyn Fn + Send + Sync>`. Bridging them is a decision about *where the `!Send` callback lives and
how its owner-local lifetime is expressed* — an ADR-grade choice with its own tests, not a wiring
detail. Recorded this way so the deferral names the real blocker rather than a question the
reference already answered.

This is a partial, not parity.

Deliverables:

1. `flui-widgets::Semantics` — **action-builder family**: typed builders for the framework-meaningful
   actions, plus a raw escape hatch.
2. `flui-widgets::Semantics` — `in_mutually_exclusive_group(bool)`, closing #1117's seam.
3. `flui-material::Radio` — publish it, so a mounted `Radio` becomes `Role::RadioButton`.
4. `flui-testing` — an action-invocation helper so a widget-level test can do the full
   attach → export → invoke → callback round trip.
5. Tests + parity-ledger updates + removal of now-stale "no builder yet" doc notes.

## The design fork (the crux)

FLUI's widget callback convention is **`Rc<dyn Fn()>`** — owner-thread-local, **not** `Send`/`Sync`
(56 callback type aliases across `flui-widgets`/`flui-material` declare `type <name> = Rc<dyn Fn…>`;
e.g. `gesture_detector.rs:18` `type GestureCallback = Rc<dyn Fn()>`, `ink_well.rs` `TapCallback`).

But `SemanticsActionHandler` is `Arc<dyn Fn(..) + Send + Sync>`. Why: the handler is stored in
`SemanticsConfiguration`, which is stored in render objects (`RenderSemanticsAnnotations.configuration`),
so the whole type must be `Send + Sync`.

**The bound is statically load-bearing, not a convention.** Verified chain (scout, `COMPLETE`):

```
impl RenderView for Semantics            crates/flui-widgets/src/semantics/mod.rs:305-307
  type RenderObject = RenderSemanticsAnnotations;      ← must satisfy the bound below
type RenderObject: RenderObject<Self::Protocol> + Send + Sync + 'static;
                                         crates/flui-view/src/view/render.rs:451
RenderSemanticsAnnotations { configuration: SemanticsConfiguration }
                                         crates/flui-objects/src/proxy/semantics.rs:18-19
SemanticsConfiguration { actions: FxHashMap<SemanticsAction, SemanticsActionHandler> }
                                         crates/flui-semantics/src/configuration.rs:101
SemanticsActionHandler = Arc<dyn Fn(..) + Send + Sync>   crates/flui-semantics/src/action.rs:217
```

Drop either bound and `SemanticsConfiguration` becomes `!Send + !Sync`, so
`RenderSemanticsAnnotations` does too, and the associated type at `mod.rs:307` fails E0277. Both
bounds are needed. `RenderObject<P>`'s own trait carries no `Send`/`Sync` (`render_object.rs:178`)
and `RenderTree` is deliberately pinned `!Send + !Sync` (`storage/tree.rs:1090`), so **the flui-view
associated-type bound is the only surviving enforcement**. This decides the fork:

- **Option A — require `Send + Sync` on the action builder only.**
  `#[must_use] pub fn on_tap(mut self, handler: impl Fn() + Send + Sync + 'static) -> Self`. Wrap in
  `Arc`. No new machinery, no lifetime risk, matches the lower-layer type exactly. Cost: a divergence
  from the crate's *majority* callback convention (56 `Rc<dyn Fn…>` aliases across the two
  callback-carrying widget crates); callers capturing
  `Rc`/`RefCell` state cannot attach actions.

  **The honest accounting on this bound, because the market survey disagrees with it.** No surveyed
  framework requires `Send + Sync` at its widget-facing accessibility API: GPUI's listener takes
  `&mut Window`/`&mut App` and carries no bound at all, and Bevy, Iced, Slint and Dioxus/Blitz each
  put the bound on a sender-holding wrapper instead. FLUI's builders do require it, and the reason is
  architectural rather than conventional: FLUI stores the handler *inside* `SemanticsConfiguration`,
  which rides in the render object, which `RenderView::RenderObject` pins `Send + Sync`. The surveyed
  frameworks store their listeners in an app-level per-frame map instead, so they never face the
  question. A caller pays for that with `Arc<Mutex<_>>` where they would write `Rc<RefCell<_>>`. That
  is a real ergonomic cliff, it is the honest cost of the existing architecture, and it belongs in the
  `## Mapping decisions` entry rather than in a footnote.

  **The bound is a minority precedent, not a novelty** (corrected twice under review; the first draft
  called it "a divergence from every other widget callback's bound", which overstates, and the second
  quoted a denominator that did not survive a recount). Measured:

  - **10 public builders** in `flui-widgets` took `impl Fn(..) + Send + Sync + 'static` before this
    change: `interaction/draggable.rs` (5), `interaction/drag_target.rs` (4), `scroll/page_view.rs`
    (1). This change adds 14 more to that family, all in `semantics/mod.rs`.
  - **56 `Rc<dyn Fn…>` type aliases** exist across the two widget crates that carry callbacks —
    `flui-widgets/src` (44) and `flui-material/src` (12). The convention is dominant but not
    unanimous. Both figures count `type <name> = Rc<dyn Fn…>` declarations under `src`, test modules
    included; an earlier draft of this plan carried a lower denominator (48, and 9 builders with
    `drag_target.rs` at 3) from a same-line-only scan that missed multi-line declarations and the
    `-> bool` bound `on_will_accept` carries. `crates/flui-widgets/ARCHITECTURE.md` §17 carries the
    two figures as measured here.

  An earlier count of 12 also swept `localization/localizations.rs` and `app/widgets_app.rs`; those
  two hits are `ReadFn<T>` type aliases inside test modules, not public builders, so they do not
  belong in the count. A derived "13%" figure appeared alongside it and is dropped: a ratio over a
  denominator that had not been recounted is exactly the shape of number that reads as evidence and
  is not. `draggable.rs:162-164` names the rationale
  outright, calling the `Arc` bounds "legacy storage shape, not a cross-thread callback contract",
  which is precisely an action handler's situation: it is stored in the config, which rides in the
  render object, constrained by `RenderView::RenderObject`. So the API shape stands unchanged; only
  the claim about it was wrong.

- **Option B — loosen `SemanticsActionHandler` to drop `Send + Sync`.**
  Would let the builder take `Rc`. Rejected: it cannot work without also dropping `Send + Sync` from
  `RenderSemanticsAnnotations`, which `RenderView::RenderObject` requires — the ripple runs out
  through `flui-rendering`'s render-object storage, not just `flui-semantics`' public type. It moves
  *against* the repo's direction of travel (Send+Sync views are an active goal).

- **Option C — bridge through an owner-local registry/handle resolved at invoke time.**
  The `Arc` handler captures a key; the `Rc` closure lives in an owner-local registry looked up at
  invoke. Invocation genuinely happens on the owner thread, so this is *sound*. Cost: a registry with
  lifetime management — entries must be removed when a widget unmounts, or it leaks and can call a
  stale callback. That is exactly the failure the issue's Risk section warns about, and the repo has
  prior scars here.

**Recommendation: Option A**, with the divergence named and justified (an action handler is a
first-class stored value invoked asynchronously by an accessibility request, unlike a pointer
callback consumed during a frame). **Confirmed** by the load-bearing chain above.

**Where the divergence is recorded (requirement, was missing).** Rule #1 owes a `## Mapping decisions`
entry for a local decision, and `crates/flui-widgets/ARCHITECTURE.md:12-13` names this exact class of
subject matter: "a widget's internal shape, **a callback bound**, a payload type". That file is
**absent from the earlier draft's Files-to-change table** and is added below. The precedent to follow
is Mapping decisions §1 (DragTarget, `:17-49`), which records the same decision shape: an `Arc`
payload diverging from the oracle's `Rc`/GC'd `State`, citing `HitTestEntry::metadata` being
`Arc<dyn Any + Send + Sync>`. A rustdoc note is not the venue; the crate's own structure section is.

**Independent confirmation from the reference that the mitigation is the right one.** Flutter faces
the same problem and solves it differently, which validates the diagnosis. Its `SemanticsAnnotationsMixin`
registers `_performTap` rather than the user's callback (`rendering/object.dart:5095-5098`), and the
comment there states why: "Registering `_perform*` as action handlers instead of the user provided
ones to ensure that changing a user provided handler from a non-null to another non-null value
doesn't require a semantics update." Its `_isDifferentFromCurrentSemanticAnnotation`
(`semantics/semantics.dart:3343`) compares `_actionsAsBits` and **never the handler closures**, so the
stable indirection is what keeps the bitmask comparison clean. Pinned by
`test/widgets/semantics_test.dart:706` (replacing `onTap` non-null→non-null leaves
`semanticsUpdateCount` at 0).

FLUI reaches the same end by a different mechanism: `SemanticsConfiguration::eq` compares handlers by
`Arc::ptr_eq` (`configuration.rs:173`), so an identity-stable `Arc` keeps the config equal across
rebuilds. Flutter hides the indirection inside the framework; FLUI exposes it to the caller as the raw
escape hatch. Same guarantee, and the burden sits in a different place. **Record this in the same
`## Mapping decisions` entry**, because "FLUI makes the caller hoist the `Arc` where Flutter does it
internally" is exactly the kind of divergence a later reader will otherwise mistake for a defect.

Two further reference facts that bound this change, both read from the pinned tag:

- **`onShowOnScreen` is not a widget callback in the reference.** `SemanticsProperties` has no such
  field, and `SemanticsConfiguration.onShowOnScreen` has zero callers in `lib/src`; Flutter supplies it
  from the render object at `SemanticsNode` construction and resolves it by a fallback branch outside
  the bitmask (`semantics/semantics.dart:5064-5072`). Its own test excludes it explicitly
  (`test/widgets/semantics_test.dart:522`). A FLUI builder for it is therefore **FLUI-only**: it is the
  only way to expose the action at all, it has no oracle case to port, and it owes a FLUI-authored test
  under rule #1(b).
- **The `in_mutually_exclusive_group` name converges on the reference.** It matches
  `SemanticsProperties.inMutuallyExclusiveGroup` and the widget ctor parameter
  (`widgets/basic.dart:7976`) exactly, so no rename is owed.


### Stale-doc finding surfaced while verifying the chain (separate escalation, not this diff)

`docs/adr/ADR-0002-engine-wide-threading-architecture.md:46` still asserts `RenderObject<P>: … +
Send + Sync` "is correctly placed" and cites `render_object.rs:142`. That supertrait was dropped in
the PipelineCell port (`storage/tree.rs:935-944` plus the `assert_not_impl_any!(RenderTree: Send,
Sync)` pin at `:1090`), so the ADR now cites a bound that does not exist. The same overstatement
appears at `crates/flui-app/src/app/ui_realm.rs:3921-3931` ("a retained cross-thread seam"): nothing
crosses a thread with the handler. Both are **reported, not fixed here** — an ADR correction is its
own reviewed change, and this diff does not touch either file.

## A second, non-obvious trap found while scouting

`SemanticsConfiguration::eq` (`configuration.rs:173`) compares action handlers by **`Arc::ptr_eq`**
— deliberately, so "same handler" is distinguishable from "a different handler". And
`RenderSemanticsAnnotations::set_configuration` returns `RenderUpdateImpact::SEMANTICS` on any
difference.

Consequence: if `Semantics::on_tap(..)` allocates a **fresh `Arc` on every build**, then every
rebuild yields a config that compares unequal → a `SEMANTICS` dirty impact on every rebuild → a
semantics re-publish per rebuild, even when nothing semantic changed. The plan must state the policy
for this and **pin it with a test** ("a rebuild that does not change semantics does not re-publish"),
rather than leave it to be discovered later.

Analysis — and the earlier draft of this paragraph called the cost "*benign*", which was a claim
without a measurement. **The word is withdrawn; here is the whole chain instead, with the one link
that is unmeasured marked as such.**

Verified: a fresh `Arc` makes `SemanticsConfiguration`'s `PartialEq` false (`Arc::ptr_eq`,
`configuration.rs:173-189`) → `RenderSemanticsAnnotations::set_configuration` returns
`RenderUpdateImpact::SEMANTICS` → `mark_needs_semantics` (`pipeline/owner/accessors.rs:1514`) → the
next `run_semantics` sets `should_build`, tries `try_graft_pass` and falls back to
`assemble_semantics_root` + `rebuild_semantics_owner` (`pipeline/owner/semantics.rs:128-145`) — a
**full-tree assembly** — whenever a mark has no anchored ancestor, which is exactly what happens
when the root itself is marked or the region never published (`try_graft_pass:728-746` returns
`false` for both).

**Unmeasured:** the magnitude. Whether a rebuild-per-frame animation that contains an action-bearing
`Semantics` pays O(anchored subtree) or a full-tree assembly per frame is a measurement question this
plan did not answer, and it is not answerable by reading. What is settled is the *shape* of the
policy, which the measurement does not change:

- pin the **impact**, not the mirror. A test asserting `update_render_object` on two separately built
  `Semantics::new().on_tap(..)` values returns `RenderUpdateImpact::SEMANTICS` pins the churn as a
  contract a future caching layer must consciously flip. The earlier draft's pin — "a rebuild that
  does not change semantics does not re-publish" — is **not** a pin of this policy: it passes with
  the churn fully present, because the published mirror diffs *translated* nodes and a new closure
  translates identically. It is a pin of the mirror, and it is kept below as exactly that, separately
  labelled, so the two claims do not stand in for each other.
- provide a **raw escape hatch** accepting an already-built `SemanticsActionHandler`, so a caller can
  hoist the `Arc` out of `build` and keep it identity-stable across rebuilds. That escape hatch is
  the documented mitigation, not an afterthought;
- do **not** invent a caching layer for the typed builders. If the impact churn is shown to matter,
  that is a separate, measured change.

## Settled after review: which actions get a typed builder (13 typed, 11 raw-only)

Found while enumerating the vocabulary, and it changes the deliverable's size. Two facts collide:

**Fact 1 — the reference exposes 21 action callbacks.** `.flutter/packages/flutter/lib/src/semantics/semantics.dart`
(`class SemanticsProperties`, line 1630) declares exactly these, in order: `onTap`, `onLongPress`,
`onScrollLeft`, `onScrollRight`, `onScrollUp`, `onScrollDown`, `onIncrease`, `onDecrease`, `onCopy`,
`onCut`, `onPaste`, `onMoveCursorForwardByCharacter`, `onMoveCursorBackwardByCharacter`,
`onMoveCursorForwardByWord`, `onMoveCursorBackwardByWord`, `onSetSelection`, `onSetText`,
`onDidGainAccessibilityFocus`, `onDidLoseAccessibilityFocus`, `onFocus`, `onDismiss`, plus
`onExpand`/`onCollapse` and a `customSemanticsActions` map. FLUI's enum deliberately dropped
`Expand`/`Collapse` (see the reserved-bit note at `crates/flui-semantics/src/action.rs:93-100`), so
**21 of FLUI's 24 variants are exactly the reference's own callback set.**

**Fact 2 — 8 of those 21 cannot reach assistive technology today.** `apply_actions`
(`crates/flui-semantics/src/accesskit_translation.rs:186-250`) carries only 16 variants. Its own doc
states the rest are "intentionally not emitted": the four cursor moves, `Copy`/`Cut`/`Paste`
(AccessKit routes those through the platform's text interface, not tree actions), and `Dismiss`.
Verified rather than trusted: AccessKit 0.25's `Action` enum has no `Dismiss` and no copy/cut/paste
members, so the comment is accurate. `semantics_action_for` (`:258`) is the inverse and returns
`None` for the same set, so they are also unroutable inbound.

The two candidate rules both give the wrong set, so the cut is derived from three gates instead. The
earlier draft's "mirror the outbound table, 16 typed" is **wrong on its own criterion**, because
`apply_actions` is the right line for *advertising* but the wrong line for *builders*: it is never
intersected with the inbound table. A builder earns its place only if the action passes all three:

- **G1 advertised** — `apply_actions` (`accesskit_translation.rs:189-237`) emits a bit. Otherwise no
  assistive technology can ever see the action.
- **G2 routed** — `semantics_action_for` (`:258-277`) maps back *and* dispatch keys on the action
  itself (`owner.rs:502`: `effective_actions_as_bits() & request.action.value()`).
- **G3 constructible** — the published node must let the AT form the request from the tree.
- **Narrowing test** — the typed builder must narrow the parameter surface. Where the honest signature
  is still `Option<ActionArgs>`, there is nothing to type and the builder is a rename, not a typing.

| Variant | G1 | G2 | G3 | Ruling |
|---|---|---|---|---|
| `Tap`, `LongPress` | Click, ShowContextMenu | ok | ok | **typed** `impl Fn()` |
| `ScrollLeft/Right/Up/Down` | ok | ok | ok | **typed** `impl Fn()` |
| `Increase`, `Decrease` | Increment, Decrement | ok | ok | **typed** `impl Fn()` |
| `ShowOnScreen` | ScrollIntoView | ok | ok | **typed** `impl Fn()` (FLUI-only, see above) |
| `Focus`, `DidLoseAccessibilityFocus` | Focus, Blur | ok | ok | **typed** `impl Fn()` |
| `SetText` | SetValue | ok | ok | **typed** `impl Fn(&str)` |
| `ScrollToOffset` | SetScrollOffset | ok | ok | **typed** `impl Fn(f64, f64)` (FLUI-only) |
| `DidGainAccessibilityFocus` | as `Focus` only | **fails as itself** | ok | **raw-only** |
| `SetSelection` | SetTextSelection | ok | ok | **raw-only** (fails narrowing) |
| `CustomAction` | bit only | ok | **fails** | **raw-only** |
| cursor moves x4, `Copy`, `Cut`, `Paste`, `Dismiss` | **fails** | **fails** | **fails** | **raw-only** |

**13 typed, 11 raw-only**, verified variant by variant against `crates/flui-semantics/src/action.rs`.

**Why `DidGainAccessibilityFocus` is raw-only, not merely inert** (this corrects the earlier draft).
It is advertised but **cannot fire as itself**: outbound it folds into `accesskit::Action::Focus`
(`accesskit_translation.rs:226-230`), and inbound it is never produced, because `semantics_action_for`
maps `Focus` to `SemanticsAction::Focus` only and the file documents the asymmetry at `:248-256`
("the legacy handler is a notification hook, not the action's implementation"). Dispatch keys on the
action, so registering `DidGainAccessibilityFocus` alone sets bit `1<<15` (`action.rs:70`) while the
inbound `SemanticsAction::Focus` tests a different bit and `resolve_action` returns
`UnsupportedAction`. **The node still advertises `Action::Focus`** throughout. That is exactly the
defect `accesskit_translation.rs:241-243` warns about ("an action advertised outbound but unroutable
inbound is a control assistive technology can see and press but that does nothing"), and it is live
today. A typed builder here would be a dead-control generator.

**Why `SetSelection` fails the narrowing test.** Its args are genuinely conditional:
`semantics_action_args_for` returns `None` when the selection's anchor or focus node is not the
target (`accesskit_translation.rs:308-312`), and the documented policy is to route without arguments
and trace the drop (`:290-293`). So the only honest signature is `impl Fn(Option<(i32, i32)>)`, which
reproduces `ActionArgs`' own optionality and drops only the action discriminant. Raw-only is the
conservative call; if it is ever typed it must be `Option`, never a fabricated pair.

**Why the 8 unroutable reference callbacks stay absent.** Verified against the registry rather than
trusted: `accesskit-0.25.0`'s `Action` enum is the complete vocabulary and has **no `Dismiss` and no
copy/cut/paste members**, so this is the platform's action set, not a FLUI gap someone will close.
Three further reasons, each sufficient:

- The behavior is already deliberately absent with a recorded rationale
  (`accesskit_translation.rs:184-188`). A widget-layer builder would launder that recorded lower-layer
  divergence into an unrecorded upper-layer promise, which is "MVP reported as parity" moved up one
  layer.
- The oracle debt is unpayable: no FLUI test can fail for `on_copy`, because a ported Flutter `onCopy`
  case would pass while the callback is never invoked. That is fake-passing by construction
  (Definition of Done §2).
- It is irreversible at a price there is no need to pay. The workspace is `0.2.0` and unpublished, so
  an `.on_copy()` added now and removed when AccessKit gains the member is a breaking change paid
  later; withholding costs nothing today.

The decisive argument, in one line: a builder that lets an author write `.on_copy(handler)` when the
framework provably cannot deliver a copy request is a lying API. An absent `on_copy` is visible and
documented; a present one that cannot fire is not. The raw escape hatch keeps all 24 variants reachable
so nothing is lost when the translation gap closes.

**`CustomAction` is raw-only, and the reason is sharper than "the label path is missing"** (settled
after review). FLUI *models* custom actions fully below the widget layer: `CustomSemanticsAction` with
id, label and hint (`properties.rs:86-95`), a `custom_actions` field on the configuration
(`configuration.rs:125`), `add_custom_action` (`:759`), and the data carried onto
`SemanticsNodeData.custom_actions` (`snapshot.rs:190`). Then `to_node`/`apply_actions` **drop it**.
Verified exhaustively: `rg "Custom" crates/flui-semantics/src/accesskit_translation.rs` returns
exactly six hits, and nothing anywhere in the crate calls accesskit's `set_custom_actions` or
`push_custom_action`. So the node advertises "this node has custom actions" with **zero enumeration**:
no ids, no labels, nothing for the AT to offer. A typed builder here would promise an id-bearing
callback that nothing outside a synthetic test can supply, which fails G3 and is the same lying-API
shape as `on_copy`.

This is a **fixable FLUI omission, not a platform limit**: `to_node` already has `data.custom_actions()`
in hand, so the enumeration is a few lines. It is nonetheless **out of scope for this change**, which
is about the widget surface: the fix lives in `accesskit_translation.rs`, and landing it here would
mean expanding a widget-layer diff into the translation layer. **Name it below and in the PR body
rather than folding it in**, and keep the builder raw-only until it lands. Flutter does enumerate
(`semantics/semantics.dart:3807`), so this is a recorded divergence with a named owner.

## Why #1118's catalog half is genuinely blocked, not merely deferred

The original plan deferred #1118 steps 2–5 with a soft reason (the ownership decision between
`InkWell` and the higher-level control). Reading the issue text and the two candidate wiring sites
gives a hard one.

`InkWell::on_tap` accepts `impl Fn() + 'static` (`crates/flui-material/src/ink_well.rs:162`) and
stores it in an `Option<TapCallback>` where `type TapCallback = Rc<dyn Fn()>` (`:123`, `:136`).
`GestureDetector::on_tap` is the same: `impl Fn() + 'static` at
`crates/flui-widgets/src/interaction/gesture_detector.rs:192`, stored as
`Rc<dyn Fn()>` (`:18`).

`Rc<dyn Fn()>` is neither `Send` nor `Sync`, so it **cannot be moved into a `SemanticsConfiguration`**
at all, under any builder bound. Option A is therefore a necessary condition for #1118, not a
sufficient one: adding `Send + Sync` builders does not let `InkWell` publish anything, because the
closure it already holds is erased to `Rc` at store time.

Wiring the catalog needs one of two ADR-grade decisions:

- **(a) Migrate the interactive-callback storage to `Arc<dyn Fn() + Send + Sync>`** across
  `InkWell` and `GestureDetector` (and the 56 `Rc` callback aliases behind them). A public API
  change rippling to every catalog call site, moving against the recorded direction.
- **(b) Option C — an owner-local handle resolved at invoke time**: the `Send + Sync` handler carries
  a key, the widget-local `Rc` closure stays in an owner-local map looked up at invoke. Sound,
  because resolution and invocation both happen on the owner thread. Cost: entries must be removed on
  unmount or the map leaks and can call a stale closure.

**The market has answered this, and the answer is (b).** A survey of eight frameworks (Compose,
SwiftUI, GTK4/libadwaita, egui, Iced, Slint, GPUI, Masonry/Xilem, Bevy, plus Dioxus/Blitz as the
cautionary case) found the same mechanism in five independent Rust implementations, and **no
framework requires `Send + Sync` at its widget-facing API**:

- GPUI's widget-facing listener is `Box<dyn FnMut(..) + 'static>` with **no `Send`, no `Sync`, no
  `Arc`** (`gpui/src/_accessibility.rs`); it takes `&mut Window`/`&mut App`, so it *cannot* be `Send`.
  The `Send` bound lives in a neighbouring `A11yCallbacks` struct whose action field is a closure
  holding an `async_channel::Sender`, and the payload that crosses is a plain `ActionRequest`.
- Bevy stores `Arc<Mutex<VecDeque<ActionRequest>>>` and drains it on the main thread; pop-os/iced uses
  `mpsc::UnboundedSender`; Slint, Dioxus/Blitz and `accesskit_winit` all hold a winit event-loop proxy.
  Every one of them holds **a sender, not a callback**.
- Resolution is by id on the owning thread, and there is no separate registry to invalidate: the
  accesskit `NodeId` *is* the toolkit's widget id (GPUI derives it from `GlobalElementId`, Masonry
  casts `target_node.0` straight into `WidgetId`, pop-os/iced does `Id::from(u128::from(...))`).

So (b) is not a fallback to be justified against (a); it is the market-standard shape, and (a) is the
one the survey found nobody shipping a retained-tree UI adopts.

**And (b) is cheaper than this plan first priced it.** The cost above — "entries must be removed on
unmount or the map leaks and can call a stale closure" — is real but is largely already paid:
`InkWellState` already holds exactly the widget-local slot Option C needs,
`tap_slot: Rc<RefCell<Option<TapCallback>>>` (`crates/flui-material/src/ink_well.rs:236`), seeded at
mount (`:276`) and refreshed on every rebuild (`:379`), and its call site already resolves
*then-current* rather than captured (`:407`). The registry entry a (b)-style bridge needs is a second
reference to a slot that exists, and the lifecycle hook that removes it is `InkWellState`'s own
unmount — a `ViewState` it already has. What is genuinely left to decide is the key's identity and
the deregistration ordering, not the storage. Priced honestly so the follow-up is not scoped as a
rewrite it is not.

**One correction the survey gives to this plan's own framing.** It describes FLUI's
`AccessibilityActionListener = Arc<dyn Fn(ActionRequest) + Send + Sync>` as *stricter than the
substrate requires*: `accesskit::ActionHandler` itself carries no bounds, each adapter imposes its
own (`accesskit_unix` needs `+ Send` and documents "All of the handlers will always be called from
another thread"; macOS takes a bare `Rc<dyn ActionHandlerNoMut>` with no bound; Windows wraps in
`Arc<Mutex<_>>`), and `Send + Sync` appears only because AccessKit itself wraps an
`H: ActionHandler + Send` in a `Mutex`. FLUI's platform boundary already picked the right shape,
because a one-line sender-holding closure satisfies it and costs nothing. Worth recording so a later
reader does not "loosen" a bound that is already load-bearing for a different reason.

Either way, wiring the catalog is a design decision with its own ADR and its own tests, which is
exactly what #1118's own "Why This Matters" argues for ("FLUI needs one coherent action semantics
surface"). The market survey now supplies the evidence for which way that ADR should go.

This also sharpens what #1118 step 1 buys: a complete public `Semantics` action surface, usable
today by any author whose closure is `Send + Sync`, and the testability seam that proves the round
trip. It does **not** make any shipped Material control activatable, and the PR must say so.

## Verified enablers (checked directly, not taken from the issues)



- **The action round trip already exists below the widget layer.**
  `accesskit_translation.rs:189` `apply_actions` maps `SemanticsAction::Tap → accesskit::Action::Click`,
  and `semantics_action_for` (`:258`) maps `Click → Tap` back. The file's own doc says the two tables
  must stay in agreement "because an action advertised outbound but unroutable inbound is a control
  assistive technology can see and press but that does nothing". So the acceptance test is buildable:
  attach → export → assert `supports_action(Action::Click)` → resolve → invoke → callback fires.
- **#1117's translation claim is true.** `accesskit_translation.rs:88-92`: `HasCheckedState` +
  `IsInMutuallyExclusiveGroup` ⇒ `Role::RadioButton`, else `Role::CheckBox`; already pinned by a
  translation-layer test at `:735`. Only the widget seam is missing.
- **`Radio` already models group membership logically.** `Radio::new(value, group_value)`
  (`radio.rs:196`) and `is_selected()` derives from `group_value` — a radio *is by construction* a
  member of a mutually-exclusive group. It publishes only `.checked(selected).enabled(interactive)`
  (`radio.rs:346-349`), so every mounted `Radio` announces as a **checkbox**. The widget knows; it
  just never says so. That is a strong, coherent justification for the fix.
- **Test precedent exists**: `crates/flui-material/tests/checkbox.rs:159-171` already does
  `laid.enable_semantics(); laid.pump(); laid.a11y_tree().expect(..).find_by_label(..)`. The Radio
  test follows the same shape but uses `A11yTree::find(Role::RadioButton)`, because `Radio` has no
  `semantic_label` builder and therefore no label to search by.

## Files to change

| File | Change |
|---|---|
| `crates/flui-widgets/src/semantics/mod.rs` | Action-builder family (13 typed + raw hatch) + `in_mutually_exclusive_group`; drop stale doc claims. **Done** |
| `crates/flui-material/src/radio.rs` | Publish `in_mutually_exclusive_group`; remove the "substrate gap" doc note. **Done** |
| `crates/flui-semantics/src/accesskit_translation.rs` | **Not in the original table, and it had to be:** `resolve_role` reordered so checkable state is tested before `IsButton` — the builder alone does not make a radio announce as a radio in the shape users write. See the composition-defect section above. The reorder is pinned crate-locally by `a_checkable_beside_is_button_still_resolves_to_the_checkable`, which sets `IsButton` alongside the checkable flags; the pre-existing `a_checkable_in_a_mutually_exclusive_group_is_a_radio_button` passes with either order and so pins the group flag, not the precedence |
| `crates/flui-semantics/ARCHITECTURE.md` | **New file, created** — the crate had none, and the role-precedence rule is its decision, not flui-material's |
| `crates/flui-widgets/ARCHITECTURE.md` | **New row, was missing.** `## Mapping decisions` entry: the `Send + Sync` callback bound (the file's own preamble at `:12-13` names "a callback bound" as its subject), the 11 withheld actions against `apply_actions`' rationale, and the "caller hoists the `Arc` where Flutter does it internally" note. Follow Mapping decisions §1's Rule/Oracle/Choice/Consequences shape |
| `crates/flui-testing/src/a11y.rs` | `invoke_semantics_action(cell: &PipelineCell, node_id, action: accesskit::Action, args)` — **not** `lib.rs`, not a node-id-only signature, and `accesskit::Action` rather than FLUI's own action enum. See below |
| `crates/flui-testing/src/lib.rs` | **New row, was missing.** The root re-export list gains `Action`, `ActionData`, `NodeId` beside the existing `invoke_semantics_action` family, so a consumer of the helper can name the request type it takes without reaching into `flui_testing::a11y` |
| `docs/runtime-contract.toml` | **New row, was missing.** The registered root-export manifest line for `flui-testing` is updated in the same change, as `AGENTS.md` requires of any touched runtime export; the checker normalizes spacing, so the line must match the emitter's form exactly |
| `crates/flui-material/ARCHITECTURE.md` | **New row, was missing.** Two paragraphs were factually wrong and are corrected rather than left standing: the "why the flag alone was not enough" paragraph named a composition (`ListTile` + `Radio`) that measurably never merges, and the `InkResponse` paragraph claimed a match with the reference where the reference publishes `Semantics(onTap: …)` and FLUI does not |
| `crates/flui-widgets/tests/semantics.rs` | Round-trip + negative tests |
| `crates/flui-material/tests/radio.rs` | **Already written and green** (4 role tests, red→green verified). Do not rewrite |
| `crates/flui-material/tests/checkbox.rs` | **Already written and green** (negative controls: a checkbox never announces as a radio button). Do not rewrite |
| `crates/flui-widgets/tests/parity/manifest.toml` | **Unchanged** — its scope excludes `material/`, and the two radio files it does carry test widgets FLUI lacks. See the parity note under Test strategy |

**The `flui-testing` helper's shape, corrected.** The earlier draft put a node-id-only
`invoke_semantics_action(node_id, action, args)` in `lib.rs`. Two defects:

- **Module.** `crates/flui-testing/AGENTS.md` assigns accessibility to the `a11y` module
  (`A11yTree`/`A11yQuery`). Action invocation is the write half of that surface and belongs beside it.
- **Signature cannot resolve.** Resolution is owner-scoped: `SemanticsOwner::resolve_action`
  (`owner.rs:459`) is a method on one presentation's owner and walks *that* owner's tree from its
  root. A node id alone does not identify a presentation, and `SemanticsActionRequest` carries only
  `node_id` + `action` + `arguments`. The helper must take the owner cell. `PipelineCell` is already
  public (`pipeline/owner/cell.rs:53`, `with` at `:78`), and `PipelineCell::with` hands out
  `&PipelineOwner`, whose phase defaults to `Idle` — exactly the typestate `resolve_semantics_action`
  is defined on (`pipeline/owner/actions.rs:12-18`). The `PipelineCell`-taking free function works
  from both harnesses with no accessor added to either, whereas a `&self` method on `HeadlessBinding`
  would be unreachable from `LaidOut`, which exposes no binding accessor.

**Take `accesskit::Action`, not `SemanticsAction`.** A helper taking FLUI's own enum lets a test
write down the action the *test* means and start from a step the platform never stands on. Every
real request is born as an `accesskit::Action` — that is what an AT sends — and only becomes
`SemanticsAction` through the `semantics_action_for` adapter. Taking the AccessKit type makes the
test cross that hop, which is exactly the hop the new builders have to survive: a typed builder that
publishes an outbound bit nobody can route backwards is the failure this whole change is about, and a
helper that skips the adapter cannot see it. It also lines the helper up with the read half beside
it — `A11yNode::supports_action` already speaks `accesskit::Action`, so the round trip reads one
vocabulary from end to end.

**Justify the helper on the right ground.** The round trip is already reachable through production
public API (`LaidOut::pipeline_owner()` → `.with(|o| o.resolve_semantics_action(req))` →
`invoke()`, plus `AccessibilityNodeId::from_u64` and `A11yNode::id()` to build the request), so
"otherwise unreachable" would be false. The real justification is lock discipline: `actions.rs:16-17`
warns that a caller holding the owner guard must release it before invoking, and `invoke` runs
arbitrary user code. Encoding that once is worth one helper. Note also that `flui-testing` has no
`publish = false`, so this is contract-bearing and additive-minor.


## Test strategy (oracle-first)

The oracle is the **same AccessKit tree a platform adapter consumes**, not an internal fixture:
`flui_widgets::testing::LaidOut::enable_semantics()` then `.a11y_tree()` →
`flui_testing::A11yTree`, which offers `find(Role)`, `find_by_label(..)` and
`A11yNode::supports_action(accesskit::Action)`.

**What that oracle does *not* cover, stated plainly.** `a11y_tree()` re-derives the tree by walking
the node configs through the translation layer on demand. It is the same *translation* the platform
adapter runs, which is why it is the right oracle for role and action-bit questions — but it is not
the *publish* path. Delivery to a live AT goes `flush` → `publish_incremental` →
`PublishedState` diff, a different computation that decides which nodes an adapter is actually told
about and when. No widget-tier test here observes it, and the equality-trap test below — which is
about whether a rebuild re-publishes — reads that path only indirectly.

The gap predates this change and is not created by it, but it bounds what the PR may claim: these
tests certify that the builders produce a correct tree, not that a real platform adapter receives
it. That is the same "shipped seams never wired" class this repository keeps finding, so the
honest scope line goes in the PR body rather than being left for a reader to assume. Closing it
needs a test at the publish seam (`PublishedState` before/after a rebuild), which is a different
harness and is **not** part of this change.

Acceptance tests (written first, must fail before the fix):
- **Action round trip**: a `Semantics` with a tap handler exports a node where
  `supports_action(Action::Click)` is true, and invoking the resolved action runs the callback.
- **Negative**: no handler ⇒ no click action. (Also: `block_user_actions` ⇒ no click action.)
- **#1117**: a mounted `Radio` resolves `Role::RadioButton` (and *not* `Role::CheckBox`).
- **The equality trap**: a rebuild with unchanged semantics does not re-publish.

**Parity ledger: the plan's original claim here was wrong, and correcting it matters.** The ledger at
`crates/flui-widgets/tests/parity/manifest.toml` is scoped to upstream `test/gestures/`,
`test/rendering/` and `test/widgets/` — verified: `grep -c '^"material/' ` returns **0**. So:

- `widgets/radio_group_test.dart` (10 cases) and `widgets/raw_radio_test.dart` (6 cases) test Flutter's
  `RadioGroup` and `RawRadio`, widgets **FLUI does not have** (`grep` finds only
  `flui-material/src/radio.rs:171`). Their cases are not what this change implements, so **`claimed`
  stays 0 and `status` stays `pending`. Raising it would be a false claim.**
- The true oracle for #1117 is `material/radio_test.dart`, which asserts
  `SemanticsFlag.isInMutuallyExclusiveGroup` on a mounted Material `Radio` (several cases, e.g. the
  `hasSemantics` blocks around lines 264, 294, 327, 352). It is **outside this ledger's scope** and is
  not represented there.
- What this change owes in its place: a FLUI test asserting the mounted `Radio` resolves
  `Role::RadioButton`, which is the replacement oracle rule #1(b) requires. The ledger is unchanged,
  and the PR says why.
- `widgets/gesture_detector_semantics_test.dart` (21 cases) is the oracle for #1118's blocked catalog
  half; it stays `pending`.

## Risks

- **The failure mode this whole change exists to avoid, and it is the market's most common one.** Two
  surveyed frameworks shipped the a11y *tree* half while leaving the *action* half unimplemented, and
  the seam looks complete from outside: Blitz/Dioxus has `ActionRequested(_req) => { // TODO }` and
  Masonry's `disclosure_button` carries `// TODO: Submit actions?` in all three input handlers with an
  empty `accessibility()` body, so its node declares no clickable action while `on_access_event`
  already reacts to clicks. A green a11y tree test does not detect this. It is the same class as this
  repo's recorded "shipped seams never wired", and it is why the acceptance test below asserts the
  *invocation*, not only `supports_action`.
- **Double registration** — if both `InkWell` and an ancestor `Semantics` add `Tap`, the merged node
  has one action but two possible handlers. This is exactly #1070 (merge precedence). Deferred here
  because this change wires no catalog widget; must be handled together when the catalog lands.
  Flutter's answer is worth carrying forward: `InkWell` wraps its `GestureDetector` with
  `excludeFromSemantics: true` (`ink_well.dart:1419`) specifically so exactly one tap action reaches
  the tree, guarding the double-fire bug of upstream #147050.
- **`Send + Sync` ergonomic cliff** (Option A) — must be documented where users will hit it, and
  recorded in `## Mapping decisions` rather than a footnote.
- **Stale-callback lifetime** — the handler is `'static` and owned by the config; it must not capture
  widget/state handles that outlive their owner. Relevant to the catalog half.
- **Semantics re-publish per rebuild** — see the equality trap above.

## Semver and behavior classification

Additive throughout, so **minor** on `flui-widgets`, `flui-material` and `flui-testing`. Checked:
`Semantics` has private fields, so method addition cannot break a downstream literal or destructure;
no `dyn` return, so no auto-trait narrowing; `#[non_exhaustive]` must **not** be added, since the type
is constructed only through `new`/`from_properties`/`from_configuration`.

**One change is not additive, and `cargo semver-checks` will not flag it.** `Radio` moving from
`Role::CheckBox` to `Role::RadioButton` is a contract change under an unchanged signature. It is the
intended fix for #1117, so it is a deliberate break to *announce*, not an accident to bury under
"closes #1117". In-repo blast radius is zero: no test other than this change's own pins the old role.


## Out of scope (named, not silently dropped)

Each of these is **recorded here and in the PR body**, so it outlives this session. None is filed:
no issue was opened for any of them, and the filing decision is the maintainer's rather than this
change's. A reader following one of these to the tracker will find nothing there for it.

- #1118 steps 2–5: the ownership decision and Material catalog wiring. The market survey now supplies
  the evidence for which way that ADR should go.
- **`to_node` drops `SemanticsNodeData::custom_actions`**, so a node advertises `CustomAction` with no
  enumeration. Corrected after tracing it: this is **not** "data already plumbed" as an earlier draft
  said. The list lives on `SemanticsProperties.custom_actions` (`properties.rs`) and reaches the
  *snapshot* node (`snapshot.rs`, populated from `effective_custom_actions()`, with tests asserting
  id/label/hint survive), but `SemanticsNodeData` is a flattened primitive payload that never gains
  the field — `SemanticsNode::to_node_data` copies roughly fifteen config fields and omits this one —
  and `set_custom_actions` appears nowhere in `crates/`. So the fix is two hops: carry the list onto
  `SemanticsNodeData` and set it in `to_node`. It also owes a decision, because
  `CustomSemanticsAction` is `{id, label, hint}` while `accesskit::CustomAction` is `{id,
  description}` — the hint has no counterpart to land in. Keeps `CustomAction` raw-only until it lands.
- **`DidGainAccessibilityFocus` is advertised but unroutable as itself.** Precisely: outbound
  `to_node` advertises `accesskit::Action::Focus` when a node holds `Focus` **or**
  `DidGainAccessibilityFocus`; inbound `semantics_action_for` maps `Focus` back to
  `SemanticsAction::Focus` only. A node holding *only* the legacy notification therefore advertises an
  action that resolves to a variant it has no handler for. The two asymmetries are each documented and
  deliberate; the hole is their interaction. A fix must either narrow the outbound `||` or give
  `semantics_action_for` the node's action set to disambiguate — it is a pure function of the
  AccessKit action today, so the second option is a signature change. A live dead-control hole in the
  same file, independent of this change. Already pinned by
  `the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set`.
- **`docs/adr/ADR-0002-engine-wide-threading-architecture.md` cites a bound that does not exist, in
  three places, and its central section describes deleted code.** Lines 46, 73 and 199 all cite
  `crates/flui-rendering/src/traits/render_object.rs:142` as carrying `Send + Sync` on
  `RenderObject<P>`; the trait there is `Diagnosticable + Downcast + 'static` and the enforcement that
  survives is `RenderView::RenderObject` (`crates/flui-view/src/view/render.rs:451`). The section
  built on it (lines 33-42) cites `crates/flui-foundation/src/binding.rs`, which no longer exists:
  `BindingBase`, `HasInstance` and `impl_binding_singleton!` are deleted, so the ADR reads as a
  proposal for work that has shipped. Not filed as part of this change's own accounting — it is an ADR
  rewrite, and rewriting it toward a conclusion is exactly the work the ADR says should not be
  pre-empted by a doc edit.
- **`crates/flui-app/src/app/ui_realm.rs`'s `SemanticsActionHandler` note is accurate, not stale.** An
  earlier draft listed it as a stale "cross-thread seam" claim; re-read, the text states the type
  correctly (`Arc<dyn Fn(..) + Send + Sync>`) and the surrounding argument is about a `!Send`
  `PipelineCell` handle no longer being capturable — which is still true. Dropped from the
  out-of-scope list.
- #1070 (merge action precedence), #1069 (transform composition), #1074 (clip-hidden algebra) —
  separate issues in the same family, not touched.
- #1070's market counterpart is now known too: AccessKit exposes `add_child_action`/`child_supports_action`
  as a container-merge primitive with no policy, and Compose's written rule is that a parent's action
  wins outright (`ActionPropertyKey` merge) while `mergeConfig` refuses to merge a child that itself
  merges. Worth citing when that issue is picked up.

