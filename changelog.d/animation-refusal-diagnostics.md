### Added

- **Refused animation commands report themselves** (`flui-animation`, ADR-0220): every public
  command that returns an `AnimationError` also emits one `warn!` on the `tracing` target
  `flui_animation::refused` (`REFUSAL_TARGET`) with `operation`, `kind` and `error` fields, so a
  refusal a widget discards is still visible. `stop` and `reset` on a disposed controller report
  at `debug`. `AnimationError::kind()` returns a stable snake-case name per variant.

### Changed

- **Refusal messages moved targets** (`flui-animation`): the per-site `"{operation}: {error}"`
  and `"retarget: {error}"` warnings on the crate's default target are replaced by the structured
  event on `flui_animation::refused`, which also covers refusals that were not logged before, such
  as a command on a disposed controller.
