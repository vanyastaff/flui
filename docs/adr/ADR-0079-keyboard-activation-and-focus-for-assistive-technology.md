# ADR-0079: Keyboard activation, intents at the primary focus, and focus for assistive technology

- **Status:** Accepted
- **Date:** 2026-09-24
- **Superseded-by:** [ADR-0203](ADR-0203-semantics-vocabulary.md), in part: the semantics focus
  action. `Focus` and `Blur` are published and routed through the widget's owner-local action
  lane (ADR-0086 §3), so the focus node never crosses the `Send + Sync` handler seam.
- **Amended:** below ("Amendment: activation keys, navigation keys and the focus indicator"),
  for §1 and the `Escape` item of "Not implemented".
- **Supersedes:** ADR-0023's resolve-at-own-position divergence (a `Shortcuts` resolves its
  intent at the primary focus), and ADR-0026 §4's nesting rule `Actions(Shortcuts(child))`,
  which that divergence forced.

## Context

The first run of real OS input on a Windows window ([`cargo xtask device windows-input`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs): a
`SendInput` click, then Tab and Enter, read back through UI Automation) found that no FLUI
control could be used from the keyboard, and that a screen reader could not follow the focus.
Four gaps stacked:

1. **Nothing answered Enter or Space.** There was no `ActivateIntent`, no binding for it, and
   `InkWell` answered only a pointer tap.
2. **An intent resolved at the `Shortcuts` widget's own position.** A button's activation lives
   in an `Actions` around its `Focus` — below the root bindings — so even with a binding the
   root could not reach it (ADR-0023 recorded this and said it had to be fixed before intents
   near the focused widget).
3. **A window with nothing focused dropped every key.** The key walk starts at the primary
   focus; there was none, so the first Tab never reached the bindings that would bring the
   focus in.
4. **Focus was invisible to assistive technology.** `Focus` published no semantics, so no node
   was focusable or focused, and the root went out as a `GenericContainer`, which AccessKit's
   filter keeps only while it is focused: the first focus move took the root out of the tree
   and UI Automation stopped walking the window.

## Decision

**1. Activation intents.** `ActivateIntent` and `ButtonActivateIntent` (Flutter's, `actions.dart`).
`DefaultFocusTraversal` binds Enter, Space and Select to `ActivateIntent` beside Tab/Shift+Tab,
as `WidgetsApp`'s `_defaultShortcuts` does (`app.dart:1263-1276`, tag `3.44.0`); nothing answers
it at the root, so an unclaimed activation key keeps bubbling. `InkWell` wraps its `Focus` in
`Actions {ActivateIntent, ButtonActivateIntent}` running the same activation a tap runs
(`activateOnIntent`, `ink_well.dart:852-855`, `:883-900`); a disabled well declares neither.

**2. Intents resolve at the primary focus.** `FocusNode` carries an opaque `NodeContext` (the
counterpart of Flutter's `FocusNode.context`, which a crate below the element tree cannot
hold), registered with the same generation-checked ownership as its key handler. `Focus`
records the `Actions` chain visible at its position there, with a real dependency on it.
`Shortcuts` reads the primary focus's chain at key time and falls back to its own position
only for a focused node no `Focus` widget hosts. So any `Actions` between the focused widget
and the `Shortcuts` takes part, as in Flutter's `ShortcutManager.handleKeypress`, and the
`Actions(Shortcuts(child))` nesting rule is no longer needed.

**3. A key target while nothing is focused.** `FocusManager::claim_unfocused_keys` asks for keys
to start their walk at a node when there is no primary focus; `DefaultFocusTraversal` claims its
own `Shortcuts` node and releases it on dispose. Claims nest — the newest live one wins, so a
nested traversal going away hands the keys back — and only a node attached to and owned by this
manager qualifies, so a key cannot reach another window's handlers. `primary_focus()` is
unchanged (`None` until a widget takes focus).

**4. Focus is published to assistive technology.** `Focus` wraps its child in a semantics
annotation that says `focusable` when its node can take focus and `focused` while it holds the
primary focus (`_FocusState.build`, `focus_scope.dart:715-729`; opt out with
`include_semantics(false)`). Only a node that can take focus is annotated: the node a
`Shortcuts` hosts its key handler on gets no render object, so the default bindings above every
tree add nothing to hit testing or to the semantics tree, and an annotation setting even a false
flag would gather its subtree into one node. A focus edge changes the flag, not the tree. The root of a window's tree is published
as `Role::Window` when nothing gave it a role (`to_published_node`, both the full and the
incremental path).

A `FocusScope` publishes no focus semantics of its own, as in Flutter, whose
`_FocusScopeState.build` adds only `Semantics(explicitChildNodes: true)` (`focus_scope.dart`,
tag `3.44.0`): when a scope's backing node itself holds the primary focus (an empty scope focused
explicitly), no node is focused for assistive technology and the root is reported. A scope does
record its `Actions` chain on its node (decision 2), since shortcuts resolve there.

## Flutter divergences

- **The unfocused key target.** Flutter's primary focus falls back to the root scope and an
  app's route scope sits under the `WidgetsApp` shortcuts, so it never has "no focus". FLUI
  keeps `primary_focus() == None` observable and names the target explicitly instead.
- **Activation keys.** Numpad Enter reaches FLUI as the same logical `Enter`, so one binding
  covers both; `GameButtonA` has no logical key in FLUI's key model and is not bound.

## Not implemented

The semantics `onFocus` action (an assistive technology asking a node to take focus) — the
handler must be `Send + Sync` and the node is owner-local; `focusable` is therefore not yet
advertised as AccessKit's `Action::Focus`, so UI Automation reports buttons as not keyboard
focusable until they hold the focus. Arrow-key directional traversal (`Escape` is bound: see the amendment).

## Amendment: activation keys, navigation keys and the focus indicator

Amends §1 and the `Escape` item of "Not implemented"; the rest stands.

- **Activation is once per physical press.** Enter and Select activate (`ActivateIntent`) on
  key-down; auto-repeat does not activate. Space sends `ActivationPressIntent` on key-down and
  `ActivateIntent` on its release (`SingleActivator::on_release`), so a control can show itself
  pressed while Space is held. A control that answers `ActivationPressIntent` consumes the
  key-down.
- **A press owns its repeats and its release.** `FocusManager` records the traversal unit that
  had primary focus at each key-down; a repeat or release reaches the focus walk only while that
  unit still has it. A release that arrives after focus moved to another unit (Space held, then
  Tab or a route push) is no key event anywhere, so no binding activates the new focus and no
  node of a covered route hears it. The press's path is told instead, through
  `FocusNode::register_on_key_cancel`, and so is it when the window loses activation with the
  key held: a held-key consumer such as a keyboard scroll ends what the press began
  (`keyboard_focus_contracts` in `flui-interaction` and `flui-widgets`).
- **Escape sends `DismissIntent`.** Only a dismissible surface answers it.
- **Back sends `BackIntent`:** Alt+Left and `BrowserBack`, or Cmd+[ and `BrowserBack` on macOS and
  iOS (Option+Left moves by word there), once per press.
- **Input modality drives the indicator.** `FocusRing` shows only while the enclosing `Focus` has
  visible focus: focus moved by a key, traversal or assistive technology (`Focus` requests it with
  `FocusCause::Assistive`), not focus set by a pointer press. `RawButton` is a Tab stop with
  `on_press`, answers the activation intents and draws the ring.

Handlers for `BackIntent` and `DismissIntent` (the navigator and modal routes) are not wired yet.

## Consequences

- Buttons and every `InkWell`-based control work from the keyboard; the first Tab into a window
  reaches a control; a screen reader sees which control holds the focus.
- `Shortcuts` is a stateful view (it reads the presentation's `FocusManager`), and
  `find_semantics_wrappers` leaves out the focus-state annotation the way it leaves out a
  `GestureDetector`'s actions-only one.
- Each behavior has a test that fails without it: `activation_tests` in `shortcuts.rs`, the
  `InkWell` Enter tests, `generic_containers_are_lifted_and_hidden_subtrees_dropped` (the
  window root is published as a `Window`, which AccessKit's filter keeps), and the
  live [`cargo xtask device windows-input`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs).
