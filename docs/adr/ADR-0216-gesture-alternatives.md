# ADR-0216: Every behavioural gesture has typed alternatives: a semantics action, a key and a single-pointer path

- **Status:** Accepted (2026-10-13); §4 records the shape it shipped with
  ([tasks](../plans/specs/flui-interaction/completeness/tasks.md); design §14, §18).
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
key or a single-pointer path. `flui-semantics` publishes custom actions to
`accesskit::Node::custom_actions`, but no locked adapter (`accesskit_windows` 0.35.1,
`accesskit_macos` 0.27.1, `accesskit_atspi_common` 0.21.0) routes them or `ShowContextMenu`
([follow-ups](../research/2026-10-12-interaction-input-followups.md) §4). In WCAG 2.2, 2.5.1
Pointer Gestures and 2.5.7 Dragging Movements require single-pointer alternatives; this record
also addresses the related 2.5.2 Pointer Cancellation (activation on release), 2.5.8 Target Size
(Minimum), 2.4.11 Focus Not Obscured (Minimum) and 2.1.1 Keyboard
([WCAG 2.2](https://www.w3.org/TR/WCAG22/)). A review checklist has not kept them; a type can.

## Decision

### 1. `GestureAlternatives` is a required argument, not a convention

Every gesture of a behavioural widget (`SwipeActions`, `Dismissible`, `ReorderableList`,
`ReorderableGrid`, `InteractiveViewer`, `ContextMenuArea`, `SelectionArea`, `AnchoredDrag`
sheets) leads to an `Intent` (ADR-0023). `GestureAlternatives` registers that one `Intent` on
three paths besides the gesture, and the widget's constructor takes it:

```rust
pub enum Alternative { VisibleControl(ControlSpec), ContextMenu(MenuSpec) }
// `ControlSpec`: the control and the `Intent` it invokes.
// `I: Intent` is typed; `Intent` carries `#[diagnostic::on_unimplemented]`.
```

1. **Semantics action** in the ADR-0203 vocabulary: zoom is `Increment`/`Decrement` with a
   numeric value; a menu is `ShowContextMenu`; swipe actions, dismiss (AccessKit has no
   `Dismiss`) and reorder moves are one `CustomAction` each, registered through
   `Semantics::custom_action(label, handler)` with an opaque `CustomActionId`. Within one
   `Semantics` a label names one action (a repeated label replaces the handler in place), so
   gesture widgets build labels that name their target ("Delete Alice", never two "Delete"
   actions in one merged node); a node that blocks user actions offers none, and a request by
   its id is refused.
2. **Key.** Defaults are per-platform `Shortcuts` in `flui-widgets`, beside the `Intent`
   (ADR-0079 resolves them at the primary focus). Esc is never destructive: it cancels, closes or
   returns to rest; during IME composition Esc goes to the IME. No default collides with Back,
   window or tab close, or text-editing keys of a focused field; a table test checks the
   collisions per platform.
3. **Single-pointer alternative** without a path or a second contact (2.5.1, 2.5.7):
   `Alternative::VisibleControl` (the default) or `Alternative::ContextMenu`.

A path or multipoint callback on a behavioural widget without an `Intent` is a diagnostic.
`GestureDetector` and `RawGestureDetector` are the mechanism behavioural widgets are built on,
not behavioural widgets: they neither take nor offer `GestureAlternatives`. A widget whose
gesture performs an operation mounts the detector inside its own alternatives and performs the
command through `CommandHandle`; the detector's taps and long presses need no path, and its
semantics actions reach assistive technology directly.
Activation completes on release and moving off cancels (2.5.2).

### 2. When `ContextMenu` is an admissible alternative

`ContextMenu` is admissible only when all of these hold; otherwise the widget uses
`VisibleControl`. `MenuSpec`'s builder requires the `Sequenced` long-press trigger and one
`Intent` per item; the remaining conditions are rows of the preset table and the input-path
matrix, because a constructor cannot check them:

- the menu opens on right-click, on long-press built as `Sequenced` (ADR-0208) whose duration is
  at least the OS accessibility floor (`SystemPreferences`, ADR-0172), and on the platform menu
  key where the preset declares one (Menu, Shift+F10); where none exists (macOS), the
  focus-revealed control is the keyboard and AT path;
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
- **Assistive pointers.** A dwell or switch device that drives a pointer of its own reports
  `PointerKind::Virtual`, with the device named by the event's `DeviceId`, so presets and
  `allowed_devices` route it to the single-pointer path. It gets no kind of its own: what
  routing needs from it (no physical contact, so no long press and no contact slop; a preset's
  `min_target`) is what every contactless pointer needs, and a consumer that must tell a dwell
  device from a gamepad cursor reads the `DeviceId`. Where the operating system turns such a
  device into mouse input (Windows Eye Control) or into accessibility actions (Android Switch
  Access, iOS Switch Control), it reaches FLUI as that input, and the alternatives' mouse and
  semantics paths serve it; `assert_gesture_alternatives` drives the single-pointer path with a
  mouse, a finger and a virtual pointer.
- **The UIA rule.** UI Automation has no custom-action surface through AccessKit, so every
  `CustomAction` is also reachable through UIA by Invoke on a visible or focus-revealed control,
  or from the context menu, and from the keyboard.

### 4. The shape

```rust
GestureAlternatives::new(Alternative) -> Result<GestureAlternatives, GestureAlternativesError>
enum Alternative { VisibleControl(ControlSpec), ContextMenu(MenuSpec) }
ControlSpec::new(GestureCommand, content).control(GestureCommand, content).placement(ControlPlacement)
MenuSpec::new(GestureCommand, presenter: impl Fn(&mut EventCx, MenuRequest)).item(GestureCommand)
GestureCommand::new(label, impl Intent).key(SingleActivator)
GestureAlternatives::handle(&self, &dyn BuildContext) -> CommandHandle   // the gesture's path
```

- **Every command sits inside the alternative.** A `ControlSpec` holds one control per command
  and a `MenuSpec` one item per command; the first is the widget's gesture command. A command
  therefore cannot exist without its single-pointer path, and `new` refuses an empty or repeated
  label (`GestureAlternativesError::{EmptyLabel, DuplicateLabel}`), which assistive technology
  could not tell apart.
- **One chain.** Every path resolves the command's intent through the `Actions` chain where the
  alternatives are mounted; the gesture reaches it through `CommandHandle::perform`, which the
  widget builds with `handle(ctx)` in its own `build`. An intent nothing answers is a
  `flui::a11y` warning on the path that tried.
- **Keys.** A command's own keys bind while focus is in the row, less any the platform reserves
  (§1.2's list, plus the Cocoa Control-letter text bindings on Apple platforms), each dropped
  with a warning. `ShowContextMenuIntent` is the menu's intent; its defaults
  (`ShowContextMenuIntent::default_keys`: Menu and Shift+F10, Menu alone on Android, none on
  Apple platforms) are root bindings of `DefaultFocusTraversal`.
- **The menu's pointer triggers.** Secondary click for a mouse, pen or trackpad; a long press,
  the hold of a `Sequenced` composition, for touch and pen. A mouse, a dwell or a switch device
  has no long press, so a mouse drag on the row meets no competitor. The long-press drags
  beneath the row are that hold's successors, not holds of their own: a finger that opened the
  menu and moves past the pan slop hands its contact to the drag, and the presenter closes the
  menu through `MenuSpec::dismiss` (ADR-0208 §3). The menu itself is the
  `MenuSpec`'s presenter, which receives the items, the cause and the pointer position;
  `assert_gesture_alternatives` finds the items it shows in the semantics tree.
- **Controls** are focusable buttons labelled by their command, drawn at least the touch minimum
  target square, after the content in reading order by default and mirrored in a right-to-left
  locale. The control a menu row reveals on keyboard or assistive-technology focus is labelled
  `WidgetsLocalizations::context_menu_button_label(target)` ("Actions for Alice").
- **Announcements** are live-region nodes until the owner's announcer exists.
- **A closure is not an intent.** `Intent` carries `#[diagnostic::on_unimplemented]`, so a
  callback passed where the gesture's intent belongs fails to compile with the reason.

## Consequences and verification

- `assert_gesture_alternatives` (`flui-testing`) drives, for each declared `Intent`, the gesture
  through `PointerScript`, the semantics action through an AT request, the key through the
  platform's keys, and the single-pointer path with a mouse, a finger and a virtual pointer. It
  asserts one identical model effect and announcement per path, Esc non-destructive, the
  controls' target size, and reachability without hover. Under `MotionPreference::Reduce`, set
  directly and derived from an animator duration scale of 0, the semantics action must reach the
  same end state within two frames, so an alternative that waits on an animation reduced motion
  does not settle fails there.
- Each behavioural widget adds its rows to the input-path matrix in its own PR; a widget whose
  gesture path lacks an alternative fails there. The shortcut-collision table lives in
  `flui-widgets` (`gesture_key_collision_table`).
- CI runs all of this headless against the in-process semantics tree and the produced AccessKit
  nodes. Delivery through UIA, NSAccessibility, AT-SPI, UIAccessibility and Android adapters runs
  only on native hosts (`cargo xtask device`), and the adapter-version check gates dropping the
  focus-revealed control slot. Upstream AccessKit support for custom actions is a contribution,
  not a dependency.

## Alternatives rejected

- **Opt-in custom semantics actions** (Flutter `CustomSemanticsAction`): nothing makes a widget
  register them.
- **Context menu as the only alternative without a focus-revealed control** while adapters
  cannot route it.
- **Hover-revealed buttons:** unreachable on touch, by AT and without a mouse.
