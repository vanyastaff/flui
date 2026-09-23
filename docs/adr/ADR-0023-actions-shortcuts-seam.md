# ADR-0023: `Shortcuts` / `Actions`, and bubbling key dispatch

- **Status:** Accepted
- **Date:** 2026-07-10
- **Related:** ADR-0026 (focus widgets and traversal; Tab is an intent built on this)

## Context

Key dispatch was flat — global handlers, then the focused node, ancestors never consulted — and
the `FocusNode::on_key_event` field that the `Focus` widget writes had no caller on the platform
path. Flutter's dispatch walks `[primaryFocus, ...ancestors]` leaf to root, combining each
node's `KeyEventResult`: `ignored` continues upward, `handled` stops and consumes,
`skipRemainingHandlers` stops without consuming (`focus_manager.dart`, `handleKeyMessage`,
`combineKeyEventResults`).

`Shortcuts` is mechanically a `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper: a matched
activator resolves to an `Intent`, dispatched through `Actions`. The ancestry walk is the whole
delivery mechanism — a `Shortcuts` above a focused field only sees a key because the field
ignored it and the event bubbled.

## Decision

**1. Dispatch bubbles.** `FocusManager::dispatch_key_event` runs global handlers first, then
walks the primary focus and its ancestors calling each node's key handler and combining results
with `KeyEventResult::combine` (Flutter's `combineKeyEventResults`). It returns whether the event
was consumed. The focus node tree mirrors the widget tree (ADR-0026), so a non-scope `Focus`
above a field is its ancestor and sees what the field ignores.

**2. `SingleActivator` and `CallbackShortcuts`.** `SingleActivator::new(key)` with
`.control()`/`.shift()`/`.alt()`/`.meta()`/`.allow_repeats(bool)`; `matches` is Flutter's
`accepts`: key-down or an allowed repeat, trigger equality, and an **exact** match on all four
modifiers. `CallbackShortcuts` binds activators straight to `Rc<dyn Fn()>` callbacks inside a
`Focus::new(child).can_request_focus(false).on_key_event(…)` wrapper, as Flutter builds it: the
first matching binding fires, and the key is handled iff one fired.

**3. `Intent` / `Action` / `Actions`.** `trait Intent: Any` is a marker keyed by `TypeId`, as
Flutter keys by `Type`; the downcast happens inside a typed wrapper, never against a view.
`Action<T: Intent>` has `is_enabled`, `invoke(&T) -> ActionOutcome` and a defaulted
`to_key_event_result` (ADR-0026 §4); `CallbackAction<T>` wraps a closure. Each `Actions` widget
layers its bindings over the enclosing chain at provide time — `HashMap<TypeId, ErasedAction>`,
a nearer scope's mapping for a type **replacing** the enclosing one — so a single
nearest-provider lookup gives what Flutter's ancestor walk gives. As in Flutter, a disabled
nearer action stops resolution at its own scope; it does not fall through to an enclosing
mapping (`actions_test.dart`, "Disabled actions stop propagation to an ancestor").

**4. `Shortcuts`.** A `Focus(can_request_focus: false)` wrapper whose handler finds the first
matching activator, resolves its intent through the captured action chain (with a real
dependency, so chain changes re-capture), and returns the action's key result: `Handled` for a
consuming action, `SkipRemainingHandlers` for a `consumes_key = false` one, `Ignored` when
nothing matched.

## Flutter divergences

- **Actions resolve from the `Shortcuts` widget's own position**, not from the primary focus
  context: a `FocusNode` does not record its element, so there is no node→element lookup.
  Visible only when an `Actions` map sits between the focused leaf and the `Shortcuts` widget.
  It forces the nesting rule `Actions(Shortcuts(child))` (ADR-0026 §4), and it must be fixed
  before text-editing intents that depend on `Actions` near the focused field.
- **One global-handler tier** before the walk, instead of Flutter's early and late tiers.

## Not implemented

`LogicalKeySet` (needs a pressed-key tracker), `CharacterActivator` (no consumer), a public
`ShortcutManager`, `ShortcutRegistrar`, `Actions.handler`, `includeSemantics`.

## Consequences

- The `Focus` widget's key handler is live, and everything above it reuses the inherited
  provider pattern and `keyboard_types` modifier flags.
- The focused node is the walk's first stop, so a field's own key handling is unchanged unless
  an ancestor also handles keys — which is the new capability.

## Alternatives rejected

- **An invoke-time ancestor walk over `Actions` scopes.** `BuildContext` cannot iterate
  providers; layering at provide time gives the same answer with one lookup.
- **A per-type fallback list of candidate actions.** It let a disabled nearer action fall
  through, which is the opposite of Flutter's contract.
