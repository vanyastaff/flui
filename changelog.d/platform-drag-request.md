### Added

- **`flui-platform`**: a decoded press names its press sequence. `PointerPress::press()` (and the
  same on every button change and on the release of that sequence) returns the `PressId` the host
  minted when the sequence began; a later press of the same contact gets another id, though its
  `PointerId` may be the same. `DragStart::press` hands it to a drag, and the drag's OS session
  starts from that press.

### Changed

- **`flui-platform`**: `OsDragRequest` holds its rules in its type. Its fields are private; it is
  built from `OsDragTerms::try_new(key, serialized, allowed)`, which refuses an empty action set,
  more representations than a `RepresentationIndex` names, and a serialized representation of the
  identity's media type (`DragRequestError`), and appends the session's identity representation
  last itself. A backend reads the request through `terms()` and `press()` and takes it apart with
  `into_parts()`; `OsDragTerms::indexed()` pairs each representation with the index its provider
  serves it under. The request names the press that began the drag (`PressId`) instead of a
  `PointerId`, which a contact reuses for its next press.
- **`flui-interaction`**: a drag whose representations cannot be offered to the OS no longer reaches
  the backend. On a desktop it is refused as it leaves its window (`MoveOutcome::Refused` with
  `DragError::Terms`) and stays in it; on the web and iOS, which start the OS session as the drag
  starts, `DragAndDrop::start` refuses it with that error, as it refuses any other at-start session
  the OS cannot take.

### Fixed

- **Windows**: an OS drag begun by a press the window no longer holds is refused instead of
  starting `DoDragDrop` under the buttons of a later press of the same mouse, pen or finger.
- **Wayland, X11, Web, Windows**: a drag offering more representations than an index names no
  longer loses its identity representation, which the hosts dropped with the ones past the last
  index.
