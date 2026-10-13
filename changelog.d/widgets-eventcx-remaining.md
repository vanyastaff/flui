### Changed

- `DragTarget::on_accept`, `on_leave` and `on_move` take `&mut EventCx<'_>` as their first argument, and `on_accept` runs before the draggable's `on_drag_end`. `on_will_accept` no longer requires `Send + Sync`. `DragTargetState::slot` returns `Rc<DragTargetSlot>`.
- Every `Semantics` action builder (`on_click`, `on_show_context_menu`, `on_scroll_{left,right,up,down}`, `on_increment`, `on_decrement`, `on_set_numeric_value`, `on_scroll_into_view`, `on_focus`, `on_blur`, `on_set_value`, `on_set_scroll_offset`, `on_action`) takes an owner-local closure receiving `&mut EventCx<'_>`; none requires `Send + Sync`. `Semantics::on_action` takes a closure receiving `&mut EventCx<'_>` and the decoded `ActionCall` instead of a `SemanticsActionHandler`, and a rebuild with a fresh closure no longer raises a semantics update. An action invoked outside its realm is dropped with a warning; a `Semantics` mounted in a detached render-object context advertises no such actions.
- `UiRealm::drain_commands` (and `drain_owner_inbox`) runs inside the realm's entry itself, so a semantics action drained by a caller that has not entered the realm still reaches its handler instead of being counted as invoked and dropped.
- `flui-testing`: `invoke_semantics_action` is now `LaidOut::invoke_semantics_action` and `Harness::invoke_semantics_action`, which run the action inside the tree's realm.

### Added

- `RenderObjectContext::{register,replace,unregister}_local_payload` and `flui_interaction::{LocalPayloadTarget, resolve_local_payload}`: owner-local state reached from `Send + Sync` render data through an interaction-lane ticket (ADR-0086 §3).
- `flui_objects::SemanticsActionRoute` and `RenderSemanticsAnnotations::{action_route, set_action_route}`.
