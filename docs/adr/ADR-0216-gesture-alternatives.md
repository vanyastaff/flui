# ADR-0216: Every behavioural gesture has typed alternatives: a semantics action, a key and a single-pointer path

- **Status:** Accepted (2026-10-13); implemented by the interaction completeness spec
  ([design](../plans/specs/flui-interaction/completeness/design.md) §14, §18)
- **Date:** 2026-10-13
- **Related:** [ADR-0023](ADR-0023-actions-shortcuts-seam.md),
  [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0172](ADR-0172-host-owned-system-preferences.md),
  [ADR-0203](ADR-0203-semantics-vocabulary.md),
  [ADR-0204](ADR-0204-synthetic-semantics-nodes.md),
  [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md)

## Context

Swipe actions, reorder, dismiss, pinch zoom and long-press menus are path-based or multipoint
gestures. Today nothing makes a widget that reacts to them also publish a semantics action, a
key or a single-pointer path: `flui-semantics` stores custom actions, but nothing publishes them
to AccessKit, and AccessKit's adapters route neither custom actions nor `ShowContextMenu`
([follow-ups](../research/2026-10-12-interaction-input-followups.md) §4). WCAG 2.2 requires the
alternatives: 2.5.1 Pointer Gestures, 2.5.2 Pointer Cancellation, 2.5.7 Dragging Movements,
2.5.8 Target Size (Minimum), 2.4.11 Focus Not Obscured (Minimum) and 2.1.1 Keyboard
([WCAG 2.2](https://www.w3.org/TR/WCAG22/)). A review checklist has not kept them; a type can.

## Decision

### 1. `GestureAlternatives` is a required argument, not a convention

Every gesture of a behavioural widget (`SwipeActions`, `Dismissible`, `ReorderableList`,
`ReorderableGrid`, `InteractiveViewer`, `ContextMenuArea`, `SelectionArea`, `AnchoredDrag`
sheets) leads to an `Intent` (ADR-0023). `GestureAlternatives` registers that one `Intent` on
three paths besides the gesture, and the widget's constructor takes it:

```rust
pub enum Alternative { VisibleControl(/* control spec */), ContextMenu(MenuSpec) }
// `I: Intent` is typed; `Intent` carries `#[diagnostic::on_unimplemented]`.
```

1. **Semantics action** in the ADR-0203 vocabulary: zoom is `Increment`/`Decrement` with a
   numeric value; a menu is `ShowContextMenu`; swipe actions, dismiss (AccessKit has no
   `Dismiss`) and reorder moves are one `CustomAction` each, registered through
   `Semantics::custom_action(label, handler)` with an opaque `CustomActionId`.
2. **Key.** Defaults are per-platform `Shortcuts` in `flui-widgets`, beside the `Intent`
   (ADR-0079 resolves them at the primary focus). Esc is never destructive: it cancels, closes or
   returns to rest; during IME composition Esc goes to the IME. No default collides with Back,
   window or tab close, or text-editing keys of a focused field; a table test checks the
   collisions per platform.
3. **Single-pointer alternative** without a path or a second contact (2.5.1, 2.5.7):
   `Alternative::VisibleControl` (the default) or `Alternative::ContextMenu`.

A path or multipoint callback on a behavioural widget without an `Intent` is a diagnostic.
Activation completes on release and moving off cancels (2.5.2).

### 2. When `ContextMenu` is an admissible alternative

`MenuSpec` is constructible only when all of these hold; otherwise the widget uses
`VisibleControl`:

- the menu opens on right-click, on long-press built as `Sequenced` (ADR-0208) whose duration is
  at least the OS accessibility floor (`SystemPreferences`, ADR-0172), and on the platform menu
  key (Menu, Shift+F10);
- the row, in focus traversal, publishes `ShowContextMenu` and one `CustomAction` per item, and
  each item is its own `Intent`;
- a focus-revealed control slot (shown on keyboard or AT focus, with `Click` and at least the
  preset's `min_target`, 2.5.8, never obscured, 2.4.11) is kept until the per-widget input-path
  matrix shows the platform adapter routes the menu;
- every platform preset declares its pointer triggers (secondary click on desktop, long press on
  touch); a table row per preset asserts at least one; where no menu key exists (macOS) the
  keyboard and AT path is the focus-revealed control;
- the menu and its items are reachable without hover.

Reorder always uses `VisibleControl` (a drag handle), plus "move up", "move down", "move to
start" and "move to end" actions.

### 3. Behaviour the alternatives share

- **One outcome.** Every path invokes the same `Intent` with the same model effect.
- **Model keys.** Actions are keyed by the item's model key, not its element, so they survive
  rebuilds and lazy-list recycling. "Move to end" in a lazy list keeps focus on the moved item
  and announces its new position. Each row has a semantics cost budget.
- **Announcements** go through the owner's announcer (ADR-0204 §4), never a platform type.
- **Localization** of labels and announcements takes parameters (item, position, count); RTL
  mirrors leading and trailing.
- **Reduced motion.** Under `MotionPreference::Reduce`, including Android's animator duration
  scale 0, alternatives reach the same end state without animation.
- **Assistive pointers.** Dwell and switch devices report their own pointer kind, so presets
  and `allowed_devices` can route them to the single-pointer path.
- **The UIA rule.** UI Automation has no custom-action surface through AccessKit, so every
  `CustomAction` is also reachable from the context menu and from the keyboard.

## Consequences and verification

- `assert_gesture_alternatives` (`flui-testing`) drives, for each declared `Intent`, the gesture
  through `PointerScript`, the semantics action through an AT request, the key through the
  platform preset's `Shortcuts`, and the single-pointer path. It asserts one identical model
  effect and announcement per path, Esc non-destructive, the revealed control's target size,
  and reachability without hover.
- Each behavioural widget adds its rows to the input-path matrix in its own PR; a widget whose
  gesture path lacks an alternative fails there. The shortcut-collision table lives in
  `flui-widgets` tests.
- CI runs all of this headless against the in-process semantics tree and the produced AccessKit
  nodes. Delivery through UIA, NSAccessibility, AT-SPI, UIAccessibility and Android adapters runs
  only on native hosts (`cargo xtask device`), and the adapter-version check gates dropping the
  focus-revealed control slot. Upstream AccessKit support for custom actions is a contribution,
  not a dependency.

## Alternatives rejected

- **Opt-in custom semantics actions** (Flutter `CustomSemanticsAction`): nothing makes a widget
  register them, which is how FLUI's unpublished custom actions arose.
- **Context menu as the only alternative:** UIA and current AccessKit adapters cannot route it.
- **Hover-revealed buttons:** unreachable on touch, by AT and without a mouse.
