### Added

- **`flui-interaction`**: `drag` sessions. A `DragHandle` (from `UiRuntime::drag_handle`) starts a
  `DragSession` of `DragItem`s with `AllowedActions` inside and outside the app; it runs in its
  window on pointer capture and escalates to an OS session when the pointer leaves the window and
  the session may reach beyond it. A `DropReceiver` found by hit test answers enters and moves
  with drop feedback, receives one leave or one drop, and redeems the Rust value only for drags
  from its own runtime ([ADR-0211](/docs/adr/ADR-0211-unified-drag-and-drop.md)).
- **`flui-platform`**: the `drag` module: `PlatformDragSource` behind
  `PlatformWindow::drag_source`, `OsDragRequest`, `DragSessionKey` and its private identity
  representation, `DragError`, and the shared `plan_os_session` decision.
- **`flui-native`**: `HeadlessPlatform::with_os_drag` and `HeadlessDragAndDrop`, which play the
  OS side of a drag, between windows or from another application, in tests.
- **`flui-foundation`**: `DragSessionId`, minted by `IdMint` and refused permanently once exhausted.

### Changed

- **`flui-runtime`**: OS drag events reach the receivers under the pointer instead of being
  dropped, and an `OwnerHost` starts the OS drag sessions a callback decided on once that
  callback is over. `RuntimeHostServices` gains `with_data_transfer` and `with_process_nonce`;
  `flui-app` gives every runtime it builds the platform's source and one nonce.
