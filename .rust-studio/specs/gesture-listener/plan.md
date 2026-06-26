# Listener widget — route raw pointer events to callbacks

Status: **SHIPPED** (2026-06-26). Implemented exactly per the plan below; the
"not yet implemented" caveats are historical. End-to-end verified: a
pointer-down within a `Listener`'s bounds fires its `on_pointer_down` callback
through the real hit-test + dispatch path (`tests/listener.rs`), and the
`RenderListener` catalog-harness test asserts its hit entry carries the handler.

## Key finding: NO design blocker

The earlier "two HitTestEntry types — type ambiguity" concern is a **false
alarm**. They live at different layers and do not fight:

- `BoxHitTestEntry` (`flui-rendering/src/protocol/box_protocol.rs:1203`) —
  `{ target_id, transform }`. Protocol-internal accumulator during the hit-test
  walk; never escapes to dispatch.
- `HitTestEntry` (`flui-interaction/src/routing/hit_test.rs:94`) —
  `{ target, transform, handler: Option<PointerEventHandler>, scroll_handler,
  cursor }`. The dispatch-ready wire type, re-exported by `flui-rendering`
  (`lib.rs:121`) so both layers speak it.

**The dispatch machinery already exists AND is wired:**
- `HitTestResult::dispatch(event)` (hit_test.rs:401) iterates `path` and invokes
  each entry's `handler`, honoring `EventPropagation::Stop`.
- `GestureBinding::dispatch_event` (binding.rs:757) is the production hub.
- Platform → `AppBinding::handle_input` (flui-app/app/binding.rs:593) →
  `GestureBinding::handle_pointer_event` → `renderer.hit_test_in_view` →
  `PipelineOwner::hit_test` (owner/mod.rs:653) → `dispatch_event`.

**The ONLY gap:** nothing populates `HitTestEntry.handler`. `PipelineOwner`'s
hit-test site (owner/mod.rs:766) builds `HitTestEntry::new(id)` with `handler:
None`. No render object carries a handler.

## Types (all exist, ready)

- `PointerEventHandler = Arc<dyn Fn(&PointerEvent) -> EventPropagation + Send + Sync>`
  (`flui-interaction/src/routing/hit_test.rs:56`).
- `EventPropagation { Continue, Stop }` (hit_test.rs:32).
- `PointerEvent` re-exported from `ui_events::pointer` (`flui-interaction/src/events.rs:104`).

## Implementation plan (~365 LOC, 5 files, additive, no inversions)

1. **flui-rendering `traits/render_object.rs`** (+5 LOC): add a default hook
   `fn pointer_event_handler(&self) -> Option<PointerEventHandler> { None }`.
2. **flui-rendering `pipeline/owner/mod.rs:766`** (+8 LOC): after building the
   entry, `if let Some(h) = render_object.pointer_event_handler() { entry =
   entry.handler(h); }`. ADDITIVE — entries without a handler keep today's
   behavior. (Note: `BoxHitTestEntry` lacks a handler field; the chosen path
   interrogates the render object at the pipeline owner rather than threading
   the handler through the protocol result — clean, no protocol refactor. The
   "handler in BoxHitTestEntry" redesign is a deferred future cycle, NOT needed
   for Listener.)
3. **flui-objects `render_listener.rs`** (NEW, ~150 LOC): `RenderListener` — a
   single-child proxy (pass-through layout/paint, model on `RenderIgnorePointer`)
   that overrides `pointer_event_handler()` to return its stored handler and
   hit-tests its child + itself. Needs a harness test (catalog rule:
   `RENDER_OBJECT_TYPES` + `harness_*`).
4. **flui-widgets `interaction/listener.rs`** (NEW, ~200 LOC): `Listener` widget
   over `RenderListener`, taking `on_pointer_down`/`up`/`move` callbacks merged
   into one `PointerEventHandler` (match on the `PointerEvent` variant).
5. **Re-exports** (+3 LOC): `PointerEventHandler` from flui-rendering; `Listener`
   from flui-widgets.

## Testing

- RenderListener harness test: hit-test at a point inside it adds an entry whose
  `handler` is `Some`.
- Listener widget test: drive a `PointerEvent` through a `HitTestResult` +
  `dispatch`, assert the callback fired (and `EventPropagation::Stop` halts).
  The dispatch path is already covered by `flui-interaction` tests; the new
  coverage is "a Listener's handler reaches the entry and fires".

## Why deferred from the 2026-06-26 session

This touches the hit-test pipeline (a critical path). It deserves the same
adversarial-review rigor the animation keystone got (harsh-critic +
async/soundness reviewers). Scoped + de-risked here for a fresh-context
implementation pass. No stub/scaffolding was landed (landing the trait hook
without a consumer would be façade-without-wiring).
