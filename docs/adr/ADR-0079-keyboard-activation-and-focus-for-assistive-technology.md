# ADR-0079: Keyboard activation, intents at the primary focus, and focus for assistive technology

- **Status:** Accepted
- **Date:** 2026-09-24
- **Supersedes:** ADR-0023's resolve-at-own-position divergence (a `Shortcuts` resolves its
  intent at the primary focus), and ADR-0026 §4's nesting rule `Actions(Shortcuts(child))`,
  which that divergence forced.

## Context

The first run of real OS input on a Windows window (`cargo xtask device windows-input`: a
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
focusable until they hold the focus. Arrow-key directional traversal and `Escape` → dismiss.

## Consequences

- Buttons and every `InkWell`-based control work from the keyboard; the first Tab into a window
  reaches a control; a screen reader sees which control holds the focus.
- `Shortcuts` is a stateful view (it reads the presentation's `FocusManager`), and
  `find_semantics_wrappers` leaves out the focus-state annotation the way it leaves out a
  `GestureDetector`'s actions-only one.
- Each behavior has a test that fails without it: `activation_tests` in `shortcuts.rs`, the
  `InkWell` Enter tests, `the_root_stays_visible_when_the_focus_moves_into_the_tree`, and the
  live `cargo xtask device windows-input`.
