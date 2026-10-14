### Added

- **`flui-widgets`**: `RawGestureDetector` mounts recognizers built by
  `GestureRecognizerFactory` values (`create` once, `update` on each rebuild,
  identified by factory type and an optional key), so a parent rebuild
  reconfigures a live recognizer without dropping the contact it tracks.
  `RecognizerContext` hands a factory the scope's arena, gesture settings and
  writer.
- **`flui-sdk`**: the custom-recognizer extension surface in `interaction`:
  the recognizer and arena-member traits, `ArenaMembership`, `PrimaryContact`,
  the arena entry and disposition, `GestureComposition`, `SequencedBranch`,
  `MotionRole`, `PanClaim`, `PointerKindSet`, `GestureSettingsProvider`,
  `cancel_all`, pointer events and `EventTime`. The facade's `interaction`
  module gains the composition, ownership and observer items it lacked.
