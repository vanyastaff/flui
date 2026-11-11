# ADR-0322: A control mounts one element for its behaviours

- **Status:** Accepted (2026-11-11)
- **Date:** 2026-11-11
- **Related:** [ADR-0023](ADR-0023-actions-shortcuts-seam.md) (the action chain a focused node
  records, which the node keeps writing);
  [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md) (keyboard
  activation and visible focus, unchanged);
  [ADR-0086](ADR-0086-signal-writes-through-event-context.md) (every callback of the node receives
  the dispatch's `EventCx`);
  [ADR-0240](ADR-0240-semantics-identifier.md) (the identifier locators select by, which the
  node's one semantics configuration carries);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (the node commits its
  states before a callback or a listener runs);
  [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) (the node's state is owner-thread
  `Rc`/`Cell`)

## Context and problem statement

A control is built by nesting one wrapper per behaviour. `RawButton::build`
(`crates/flui-widgets/src/interaction/raw_button.rs`) nests `Semantics`, `GestureDetector`,
`Actions`, `Focus` and `FocusRing` around its child, and each wrapper expands again: a stateful
element, an inherited element that hands a value to descendants, a semantics annotation, a pointer
listener, two decorated boxes. Measured at `main` `da69efa` (Linux x86_64, release, headless,
semantics off):

| Mounted | Elements | Render nodes | Allocations | Bytes |
|---|---|---|---|---|
| `Text` | 3 | 1 | 105 | 9 KB |
| `RawButton` holding that `Text` | 18 | 8 | 421 | 46 KB |

The fifteen elements the button adds describe one thing: assistive technology already sees one
node, because the outer annotation is a container the inner ones join. `InkWell`
(`packages/flui-material/src/ink_well.rs`) adds a `MouseRegion` and a `CustomPaint` to the same
five and schedules a rebuild of its element on every hover, press and focus change. Every Material
and Cupertino control repeats the shape, so the cost is per control on every screen, and the
states a style needs arrive by rebuilding.

`crates/flui-view` cannot express the alternative today: a `RenderView` has no state, a
`StatefulView` has no render object, and only an `InheritedElement` can publish a value to its
subtree, so each of the three needs its own element. This record decides what a control of the
catalogs mounts and the two capabilities of `flui-view` it rests on. What does a control with
several behaviours mount?

## Decision drivers

- The entry [An interactive widget costs eighteen elements](../gaps.md#an-interactive-widget-costs-eighteen-elements)
  closes on counted numbers: a button adds to its child at most 2 elements, 1 render node, 40
  allocations and 3 KB.
- The catalogs do not wait for the last kilobyte. The Material and Cupertino controls move to the
  node at a migration threshold counted by the same scenario: 2 elements, 1 render node, at most
  40 allocations and at most 5 KB. The entry's numbers above stay the goal.
- Nothing a user or a tool observes changes: the accessibility tree, the gesture arena's
  outcomes, focus traversal, action resolution, what locators and `inspect` select.
- A disabled control is disabled in one place. A control that is disabled for the pointer and
  still focusable, or announced as enabled, has shipped in other frameworks' catalogs as a class of
  defect.
- Hover, press and focus change what is painted, not what is built: a style reads them at paint.
- The frame path is synchronous and owner-thread; no lock on state `perform_layout` or `paint`
  touches.
- Rules are types (ADR-0303): what the node forbids, it does not offer a method for.
- The public surface is Flutter's and does not change: an application and a control's author
  write `GestureDetector`, `Focus`, `Actions`, `Semantics`, `MouseRegion`, `FocusRing` and the
  control widgets, composed as widgets. FLUI departs from Flutter where Rust requires it or a
  measurement pays for it, and never in how a widget is written.
- `flui-view` grows general capabilities, each with a Flutter counterpart, not a hidden element
  kind for one widget.

## Considered options

1. The catalogs' controls mount one private element and one render object for all their
   behaviours; the public widgets are unchanged.
2. Keep the wrappers and make each one cheaper.
3. Keep the wrappers in source and fuse adjacent ones into one element at mount.
4. A public node or modifier chain that carries behaviours, which controls and applications use
   in place of the wrappers.

## Decision

Chosen option: 1, because it is the only one that reaches the counted numbers while the
behaviours stay one implementation each and the way a widget is written stays Flutter's. A
framework's own controls collapsing behaviour inside themselves is what Flutter does where it
counts (`EditableText`, `Scrollable` and `RawScrollbar` own focus, gestures and semantics in one
state); here the collapse reaches the element and the render object.

### §1 The node is private to the catalogs

```rust
// flui_widgets::interaction::node: #[doc(hidden)], in no prelude, not re-exported by the facade
#[derive(Clone)]
pub struct InteractiveNode { /* child, enabled, one Option per behaviour */ }
```

`InteractiveNode` is not a widget of the framework's API. No guide, example or template names
it, and an application cannot reach it through `flui::prelude` or the facade. It is `pub` only
because the catalogs are separate crates: Flutter keeps such a type library-private inside one
package, and `flui-material` and `flui-cupertino` are not in `flui-widgets`' crate. The module is
`#[doc(hidden)]` and outside the API contract of ADR-0089; the official catalogs are its only
callers, and it may change in any release.

Its parts are the wrappers' own configuration types, not a second vocabulary: the semantics
properties `Semantics` takes, the recognizer slots of `GestureDetector`, what `MouseRegion`,
`Focus`, `Actions` and `FocusRing` take, and a `WidgetStatesController` (§3). It takes no colour,
padding, size, shape or per-state style and never will: appearance belongs to the decoration
widgets and the theme (`ButtonStyle`, `WidgetStateProperty`, `Theme`).

The node mounts one element (§4) and one render object, `RenderInteractive` in `flui-objects`: a
single-child box of its child's size. It is made of parts, each the value of the render object
that carries the behaviour alone: a `RenderListener` (always present: where the object is hit and
its pointer targets), an optional `RenderMouseRegion`, an optional `RenderSemanticsAnnotations`
and an optional `FocusRingStyle` painted as a foreground. The views that configure those objects
configure the parts (`Listener` and `Semantics` through their own `RenderView` impls), and the
object forwards each question of `RenderBox` to the part that answers it, so no rule of a wrapper
is written twice. A part that is absent registers nothing, describes nothing and paints nothing.
The object publishes its identity into a `SubtreeAnchor` while mounted, which is how its focus
node is measured. The element's state holds what the wrappers' states hold: the `FocusNode` and
its attachment, the recognizers, the action chain, the states controller.

The focus logic (node, configuration, attachment, reparenting, replacement, autofocus, the rule
that only the current attachment may focus the node from assistive technology) is one
`FocusHost` (`crates/flui-widgets/src/interaction/focus_host.rs`) that `Focus` and the node both
own an instance of. Its owner forwards the lifecycle hooks and says how the node is measured: an
anchor over the child for `Focus`, the render object itself for the node. Recognizer mounting is
one `RecognizerHost` (`interaction/raw_gesture_detector.rs`) in the same way, owned by
`RawGestureDetector` and by the node. One implementation, two hosts.

### §2 Enablement has one source

The node's `enabled` is the only switch. While it is `false`:

| Part | Effect |
|---|---|
| semantics | the configuration says disabled and offers no action |
| pointer | no recognizer joins an arena; the hit-test behaviour is unchanged, so a disabled control still covers what is beneath it |
| hover | `on_enter`/`on_hover` are not called, the cursor is not applied |
| focus | the node cannot request focus and gives up the focus it holds, as `can_request_focus(false)` does today |
| actions | every bound mapping answers `is_enabled` `false` and stays declared, so the intent stops at the control and does not activate an enclosing one |
| states | `WidgetState::Disabled` is set; `Hovered` and `Pressed` are cleared |

The semantics part has no `enabled` of its own, and the focus part's `can_request_focus` and
`Action::is_enabled` are conjoined with the node's switch, never a second source: a control is
enabled for an intent when the node is enabled and the action is. The node's own description
reads no bound action (its click is its tap's, its focus its focus node's), so it listens to no
`Action::enablement_changes`. A description that does offer a command through a bound action names
that action's notifier as a refresh source, as `Semantics::refresh_on` and the gesture alternatives
do today, and is described again on the signal without a rebuild.

Disabling a control during a contact cancels its recognizers (`cancel_all`), then clears the
states, then runs the cancel callbacks.

### §3 Interaction states are one value, read at paint

A control gives its node a `WidgetStatesController` (`crates/flui-widgets/src/widget_state.rs`):
its own, or the one an application handed it (`RawButton::states_controller`). The node writes
five states of `WidgetStates` to it and no other part of the tree writes them for that control. A
node given no controller writes none and keeps none: its only reader would be its own focus ring,
which reads the focus node.

- `Hovered`: a pointer is inside the mouse region, from the first one in to the last one out (a
  mouse and a hovering pen are two);
- `Pressed`: a contact a recognizer tracks is down and not yet rejected;
- `Focused`: the node or a descendant of it holds the primary focus, as `Focus::on_focus_change`
  reports it;
- `FocusVisible`: the node holds focus and focus is shown (ADR-0079);
- `Disabled`: §2.

`Focused` is "holds focus, by any cause" and `FocusVisible` is the focus a user can see; the
readers that mean "show it" (state-layer overlays, rings, halos) resolve on `FocusVisible`.
`WidgetStates` is a `u16`.

The value is a `Cell` behind the controller, and the controller is a `Listenable`. A reader at
paint (a state layer, a Cupertino highlight) holds the controller, reads `value()` in `paint`, and
returns it as its repaint listenable, so a hover repaints that render object and rebuilds nothing.
A reader at build subscribes with `listen`, as today. The node sets the cell, then notifies
(ADR-0287); a listener that panics is handled by the notifier's first-failure custody, and the
value it was notified of stays committed.

When the element unmounts, or the control hands the node another controller, the node clears the
states it wrote from the one it stops writing to, so a shared controller keeps no state of a
control that is gone. A controller given later starts at what the node is: the five states are the
node's, so one that arrives holding any of them keeps only what is true of this control, and a
press or a hover under way follows its contact or its pointers to it.

The two focus edges are delivered each on its own: a handler of one that panics does not cost the
other, and the first failure is the one resumed (ADR-0281).

The node may be built first with the states written and the wrappers' rebuild-on-change readers
kept; moving a reader to paint does not change this shape.

### §4 `flui-view` has a render view with state

```rust
// flui_view
pub trait StatefulRenderView: Clone + 'static + Sized {
    type Protocol: Protocol;
    type RenderObject: RenderObject<Self::Protocol> + 'static;
    type State: RenderViewState<Self>;
    fn create_state(&self /* the arguments `StatefulView::create_state` takes */) -> Self::State;
    fn create_render_object(&self, cx: &RenderObjectContext<'_>) -> Self::RenderObject;
    fn did_unmount_render_object(&self, cx: &RenderObjectContext<'_>, object: &mut Self::RenderObject) {}
}

pub trait RenderViewState<V: StatefulRenderView>: 'static {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {}
    fn did_change_dependencies(&mut self, cx: &dyn LifecycleContext) {}
    fn did_update_view(&mut self, old: &V, new: &V) {}
    fn update_render_object(&self, view: &V, cx: &RenderObjectContext<'_>, object: &mut V::RenderObject) -> RenderUpdateImpact;
    fn provide(&self, view: &V, cx: &dyn BuildContext, values: &mut Provided<'_>) {}   // §5
    fn child(&self, view: &V, cx: &dyn BuildContext) -> impl IntoView;
    fn deactivate(&mut self) {}
    fn activate(&mut self) {}
    fn dispose(&mut self) {}
}
```

It is an element kind, `Element<V, Variable, StatefulRenderBehavior<V>>`, with the lifecycle
of a stateful element and the render-object custody of a render element; the two behaviours
share that custody (`flui_view::element::render_custody`) and the containment of a state's
hooks. Presentation capabilities are acquired in `init_state` and `did_change_dependencies`
only (ADR-0078).

The view creates the render object, not the state: the object is created and adopted at mount,
and `init_state` runs at the element's first build, so at mount the state holds nothing it
acquires there. Putting the constructor on the view makes that order a signature instead of a
rule to remember. The first build runs `init_state` and then `update_render_object` in the
same frame, before layout, so the object never lays out without its state's configuration.

The view also releases it. What the object holds in the owner's runtime is data: the targets
registered for it (ADR-0027). `did_unmount_render_object` unregisters them from the object
after the state's `dispose`, and for an element whose state never initialized, so a target
registered at any point is released on every way out.

A build runs `update_render_object`, `provide` and `child`, in that order, so the child mounts
under what is published and next to an object that is already configured. A state change that
affects the render object schedules the element through its rebuild handle; the reconciler skips
a child whose view is equal to the one it holds. A change that only repaints goes to the render
object through a handle the state gave it in `update_render_object`, and schedules no element.
The state builds one child: the node of §1 is a single-child box, and a second user that needs
several brings the change with it.

### §5 An element publishes values to its subtree

```rust
// flui_view
pub trait ProvidedValue: 'static {
    fn update_should_notify(&self, old: &Self) -> bool;
}

pub struct Provided<'a> { /* the element's slot */ }
impl Provided<'_> {
    pub fn value<T: ProvidedValue>(&mut self, value: T);
}
```

`provide` exists, with an empty default, on `StatelessView`, `ViewState` and `RenderViewState`.
It runs as the first part of each build of its element and receives the build context, so what
it publishes may be derived from what it reads (an `Actions` layers its bindings over the
enclosing chain) and a signal it reads rebuilds the element, as a read in `build` does
(ADR-0074). The hook writes into a staging list: a panic in it, or in an `update_should_notify`,
leaves the published values as they were and is recorded against the element as a `provide`
failure. The values a run replaces are dropped after the new ones are published and their
dependents scheduled, so a `Drop` that panics loses no notification.

A value is keyed by its type in the node's inherited scope (`ElementNode::inherited`), the map
`InheritedElement` registers in, so a descendant's `ctx.depend_on::<T, _>(..)` finds the nearest
provider of `T` in O(1) whichever kind of element it is. An element's own reads resolve above
it, so a provider never finds itself, and a state that publishes from its first build (its
`init_state` runs there, not at mount) enters the scope before its children mount. Each
published type owns one field of the dependent's field set, so a value that changes wakes the
readers of that value only. Dependents are recorded in the same ledger an inherited element
keeps, and are notified by the same path:

- after each run of `provide`, a value whose `update_should_notify(old)` is `true` notifies its
  dependents, exactly as `InheritedBehavior::on_view_updated` does;
- a subtree moved under another provider re-resolves its scope and its dependents receive
  `did_change_dependencies`, by the routine that re-scopes a moved subtree today;
- unmounting the provider releases its dependents by the routine that releases an inherited
  element's;
- a run of `provide` that publishes a different set of types than the previous run re-scopes the
  element's subtree and wakes every descendant that read an inherited value, since the
  provider it resolves may have moved. The node does this when a behaviour is added or removed.

`FocusParent` (the node descendants attach their focus nodes under) and `EnclosingActions` (the
layered action chain) are such
values. The node provides both from its one element. `Focus` and `Actions` provide them from
their own elements, and their private inherited views, `FocusParentProvider` and
`ActionChainProvider`, are deleted with their elements.

### §6 What must not change

- **Accessibility.** The node assembles one `SemanticsConfiguration` in a fixed order: the
  semantics properties; then the actions the enabled recognizers give (tap, long press, scroll) and
  the gesture alternatives; then `focusable`, `focused` and `on_focus` from the focus part. The
  result is the node the container annotation produces today.
- **Gestures.** The recognizers are the ones `GestureDetector` builds, mounted by the same
  factories against the same arena scope and settings, entered in the same order on a pointer
  down. A control inside a `Scrollable` and a control inside another control therefore contest as
  they do today.
- **Keyboard.** Activation and `on_key_event` reach the node through its focus node and its
  action chain (`chain_at`), as ADR-0079 has them. The pointer part takes no key.
- **Tools.** A locator selects by identifier, role and name and checks actionability by semantics
  and hit test; one node answers both. `RenderInteractive`'s `Inspect` lists its parts, and the
  node's creation site is the control's.

### §7 The public widgets are the API, and a catalog control does not nest them

`Semantics`, `GestureDetector`, `MouseRegion`, `Focus`, `Actions`, `FocusRing` and
`WidgetStatesController` are how interactive behaviour is written, by applications and by anyone
who writes a control outside the official catalogs. Nothing fuses adjacent wrappers, and no rule
of `AGENTS.md` "Current forms" steers a widget author away from them. `RawButton::build` returns
the node in place of its five wrappers, with the same constructor and methods; the controls of
`flui-material` and `flui-cupertino` follow, each family in its own change, and none changes its
public API for it.

### §8 An action route and a control's description exist while the window collects semantics

A window collects semantics while someone reads them: assistive technology is attached, or an
agent or a test holds the tree (`flui-runtime`'s `SemanticsHost` counts the holders, and
`reconcile_semantics_enablement` carries the answer to the pipeline at the start of a frame). The
view layer follows the same answer. `BuildOwner` holds it, every `RenderObjectContext` carries it,
a `Semantics` registers the route of its actions only while it holds, and a control's node builds
its semantics part, the description and the handlers it offers, only while it holds. The node's
view carries what the description is made from (a role and an identifier), not a built
`Semantics`, so the copies of the view the tree keeps stay small:

```rust
impl BuildOwner {
    pub const fn collects_semantics(&self) -> bool;
    /// A change marks every live element dirty; returns whether any was.
    pub fn set_collects_semantics(&mut self, tree: &mut ElementTree, collects: bool) -> bool;
}

impl RenderObjectContext<'_> {
    pub const fn collects_semantics(&self) -> bool;
}
```

- A presentation starts with the answer `false`, and the runtime sets it beside the pipeline's
  flag. A tree mounted outside a runtime (`BuildOwner::new`, a detached context) has the answer
  `true`, so its semantics are complete without anyone turning them on.
- A change of the answer costs one full build: every element is marked dirty, each render view's
  configuration is pushed again, `Semantics` registers or releases its route in that pass, and a
  node builds or drops its semantics part, which its render object holds out of line.
  The change is rare, and the alternative, a dependency on an inherited value, was measured: it
  costs every node with an action two allocations and more bytes than the route it saves.
- The tree a window publishes after semantics are turned on over a mounted tree is the tree of a
  window that collected from the start, and its actions reach their handlers.
- Nothing changes for a widget author: `Semantics::on_click` and the other builders are written
  as before, and a locator or an accessibility check in a test turns semantics on as before.

### Flutter counterparts

| Here | Flutter | Why it differs |
|---|---|---|
| The private node behind a control | a control's `State` composing `Focus`, `Actions`, `MouseRegion`, `GestureDetector`, `Semantics` (`InkResponse`, `FocusableActionDetector`) | measured: fifteen elements and seven render nodes per control (table above); the widgets an author writes are the same |
| `WidgetStates`, `WidgetStatesController`, a reader that repaints, `RawButton::states_controller` | `WidgetState`, `WidgetStatesController`, `WidgetStateProperty`, `statesController` of `InkWell` and `ButtonStyleButton` | none |
| `WidgetState::FocusVisible` | `FocusManager.highlightMode` and `FocusableActionDetector.onShowFocusHighlight` | one value a painter reads, where Flutter has a state and a separate callback |
| `StatefulRenderView` (§4) | a `RenderObjectElement` subclass that keeps state, which any Flutter library may write | FLUI's elements are one generic `Element<V, Arity, Behavior>`, so the capability is a behaviour kind and not a subclass |
| `provide` (§5) | `Element.updateInheritance`, which any element may override to enter `_inheritedElements` | the same mechanism, typed: the value type is the key and `ProvidedValue` carries `updateShouldNotify` |
| `collects_semantics` (§8) | `SemanticsBinding.semanticsEnabled`, kept on by `SemanticsHandle`s from `ensureSemantics`; `PipelineOwner.semanticsOwner` is absent until one is held | Flutter's semantics callbacks live in the widget's configuration, so nothing is registered per node; FLUI's handlers are owner-local and reach the render tree through a route, which is what is deferred |

`InheritedView` remains the form in which an application or a widget hands a value down, as
`InheritedWidget` is in Flutter. `StatefulRenderView` and `provide` are for a widget that owns a
render object or already owns the element the value comes from.

### Consequences

- Good, because a button adds two elements and one render node to its child, and a screen of
  controls shrinks by the same factor in elements, render nodes and mount allocations.
- Good, because nothing an application or a widget author writes changes.
- Good, because hover, press and focus repaint a state layer without rebuilding an element.
- Good, because a disabled control cannot be half disabled: there is no second switch to forget.
- Good, because `Focus` and `Actions` each lose an element through §5.
- Bad, because a control written outside the official catalogs composes the wrappers and keeps
  their cost; closing that gap is option 3 over this node, a later decision.
- Bad, because the node is a `pub` item that is not API: its privacy is a convention the
  documentation and the facade keep, not the compiler.
- Bad, because `RenderInteractive` implements hit testing, semantics and paint that five small
  render objects split today, and the focus and recognizer logic must be extracted into hosts
  before the node exists.
- Bad, because a `WidgetStateProperty` resolver ported from Flutter that shows focus on `Focused`
  shows it after a click too; it has to resolve on `FocusVisible`.

### Confirmation

- `crates/flui-widgets/tests/perf.rs`, `perf_raw_button_over_text`: the counted elements,
  render nodes, allocations and bytes a `RawButton` adds to its `Text` are within the entry's
  numbers.
- `raw_button_a11y_tree_matches_wrappers`: `a11y_tree()` of `RawButton` equals, byte for byte, the
  tree of the same button composed from the public wrappers, enabled and disabled, focused and
  not; the AT-SPI recording of the button row is unchanged.
- `raw_button_in_scrollable_contests_like_gesture_detector` and
  `nested_raw_button_contests_like_gesture_detector`: a table of contacts (tap, drag past slop,
  long press, cancel) gives the same winner, the same callbacks and the same order as the
  wrappers.
- `disabled_raw_button_is_disabled_in_every_part`: one switch, the six rows of §2, including a
  disable during a contact. The pointer and
  action rows of a control that keeps its callback while disabled are the node's `switch_tests`
  (`crates/flui-widgets/src/interaction/node.rs`).
- `hover_repaints_without_rebuilding`: a hover over a control whose child reads the controller at
  paint (a `CustomPainter` that returns it from `repaint`) builds no element, lays out no node and
  paints the reader's repaint boundary alone.
- `crates/flui-view/tests/provided_values.rs`: `provided_value_change_notifies_like_inherited_element`,
  `provided_value_follows_a_moved_subtree` and `provided_value_unmount_releases_dependents` run one
  table over an inherited view, a stateless view that publishes and a state that publishes;
  `provided_set_change_rescopes_the_subtree` (an update that drops one type and adds another while
  readers of both are mounted), `a_provide_panic_is_recorded_and_keeps_the_frame`,
  `a_replaced_value_that_panics_in_drop_keeps_its_notification` and
  `a_signal_read_in_provide_republishes_on_write` run over the
  two that publish.
- `crates/flui-view/tests/stateful_render_view.rs`:
  `a_stateful_render_view_is_one_element_with_its_state_and_render_object`,
  `a_scheduled_state_change_updates_the_render_object`,
  `a_new_view_reaches_the_state_before_the_render_object`,
  `unmount_disposes_the_state_and_removes_the_render_object`,
  `a_dispose_panic_still_removes_the_render_object`,
  `a_state_that_failed_to_initialize_is_not_disposed`,
  `an_update_panic_replaces_the_element_and_the_next_frame_runs` and
  `a_state_hook_failure_is_recorded_against_its_hook`.
- `semantics_turned_on_later_publish_the_tree_of_semantics_on_from_the_start`: a window that
  mounted without semantics and one that collected from the start publish the same tree, a click
  requested through the platform listener reaches its handler on a `Semantics` and on a
  `RawButton`, and both hold again after semantics are turned off and on.
- `perf_semantics_action_over_text`: the allocations and bytes a `Semantics` with one action adds
  to its child in a window that collects no semantics.
- The node's module is absent from the facade and from every prelude: the facade's public-API
  snapshot.
- The `render_object_harness` row of `RenderInteractive`.

## Pros and cons of the options

### 1. A private node behind the catalogs' controls

- Good, because the cost of a control stops depending on how many behaviours it has.
- Good, because the authoring style is untouched and enablement and states get one owner.
- Good, because the wrappers' logic is reused through hosts, not rewritten.
- Bad, because it needs two additions to `flui-view`, a wide render object, and a doc-hidden
  seam between crates.

### 2. Keep the wrappers and make each one cheaper

- Good, because no control changes.
- Bad, because five wrappers are at least five elements whatever each costs; the element count
  the entry closes on is out of reach.
- Bad, because enablement stays spread over three wrappers and states still arrive by rebuild.

### 3. Fuse adjacent wrappers at mount

- Good, because source code keeps the familiar nesting and third-party controls gain too.
- Bad, because what mounts no longer follows from what is written: a key, a `Builder` or a theme
  wrapper between two behaviours silently restores the cost, and `inspect` shows a tree no source
  line names.
- Bad, because the fused element needs the node anyway; the fusion is added on top of option 1,
  not instead of it.

### 4. A public node or modifier chain

- Good, because every author gets the cheap shape directly.
- Bad, because it is a second way to write a control beside the widgets, in the style of GPUI's
  `div().on_click()` or Compose's `Modifier`, and the catalogs would teach it by example. FLUI's
  authoring style is Flutter's.
- Bad, because a behaviour node invites style methods, and an ordered chain makes the order of
  behaviours a property of each call site, where §6 needs it fixed.

## More information

- The measurement is a counted probe over `HeadlessBinding` with a counting global allocator; the
  numbers and the closing condition live in `docs/gaps.md`.
- Revisit option 3 if controls written outside the catalogs show the same cost at a scale that
  matters.
