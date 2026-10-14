### Added

- **Gesture ownership table** (`flui-interaction`, ADR-0208): drags and scale claim a moving
  contact with `GestureArenaEntry::claim_motion`, and the arena gives it to one owner by the
  displacement accumulated since Down: a scale keeps it, a pan yields to a drag whose axis takes
  at least cos 45° of it, the larger axial component wins and a tie goes to the innermost member.
  `MotionRole` names a member's part; `ArenaMembership::with_motion_role` declares it.
- **`PanClaim`** (`flui-interaction`) and the **`PanClaimRegion`** widget (`flui-widgets`): a
  subtree declares which axes outer drags may take from the pans inside it, as CSS `touch-action`
  does (`GestureArena::with_pan_claim`).
- **`allowed_devices`** on every built-in recognizer builder and on `GestureRecognizer`
  (`PointerKindSet`, re-exported by `flui-interaction`): `RecognizerSet` refuses a Down of another
  device kind before admission.
- **Nested and sequenced composition** (`flui-interaction`): a branch composes again;
  `GestureArena::sequenced()` returns a `SequencedBranch` whose `hand_off` gives a contact won by
  a hold (long press) to its successor (drag). `CompositionKind` and
  `GestureArena::composition_path` report the relations.
- **Composition delay ceiling**: a composition whose waits on one path exceed
  `max_resolution_delay` returns `CompositionError::DelayCeilingExceeded`; authored values are
  summed, never accessibility floors (`GestureSettings::authored_long_press_timeout`).
  `GestureArena::with_settings` sets the profiles a handle checks against; `GestureArenaScope::of`
  applies the scope's.
- **`CancelReason::Claimed`** (`flui-platform-api`): when a recognizer that claimed a continuing
  contact wins, every other target on the contact's route receives one `Cancel` with this reason
  and nothing more of the contact.
- **`flui_testing::assert_ownership_table`**: every cell of the table, for touch and mouse.

### Changed

- **`GestureCompetition` is `GestureComposition`** with variants `Exclusive` and
  `RequireFailure` (was `RequireFirstFailure`); `CompositionError::AlreadyComposed` is replaced by
  `DelayCeilingExceeded` and `Closed`.
- **A drag holds until its slop**: a drag that wins without competition starts only once its
  contact crosses the slop on its axis, from Down, so the first update carries the motion so far;
  a drag accepted at release without that motion reports `on_cancel`, not a start.
- **Pan and axial drags no longer race**: on one contact, a pan and an axial drag are decided by
  the ownership table rather than by which crossed its slop first.
