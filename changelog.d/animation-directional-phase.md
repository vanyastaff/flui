### Changed

- `AnimationStatus::is_running` and `is_stopped` are renamed to `is_directional` and `is_settled`. Status describes a directional phase; it does not prove run ownership, endpoint arrival or successful completion. Use `Animation::is_animating`, the value and the run future for those facts.
