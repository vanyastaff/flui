### Fixed

- Commit a driven controller's new clock binding before retiring the outgoing registry; preserve the first delivery failure and keep subsequent runs usable.
- Close a driven controller and deliver cancellation before retiring its registry; capture destruction cannot admit a replacement run during owner disposal.
- Preserve controller status and run-delivery order during reentrant animation changes, finish healthy status listeners after a panic, and skip listeners removed or disposed during delivery.
- Continue ticking remaining Vsync controllers and child registries after a contained failure; retain the first failure through wrapper relays and capture retirement.
- Preserve animation progress when mounted implicit widgets and open navigator routes change their ambient frame registry; release their registration on unmount.
- Refuse bound `StateCell` and `StateHandle` writes during build before changing values or invoking update closures; a later admitted write still schedules its rebuild.
- Hold animation samples while a presentation is hidden or frames are disabled, then catch up on the first visible frame.
- Refuse exhausted animation run identities permanently; exhausting sample identities cancels the active run without publishing a stale sample.

### Added

- Per-window agent `motion` requests set playback rate, pause, step, and inspect the accepted clock state through protocol 0.2.
- SnackBar display timers pause while hovered and resume with their remaining duration; unmounting a hovered presenter releases its pause.

### Changed

- Animation listeners, run continuations and scheduler callbacks accept owner-local captures. UI state stays on its owning thread; frame and task wakers retain their cross-thread role.
- Animation controllers use a builder with validated bounds. `build_on` returns a `DrivenController` that owns registration and cancellation; controller clones observe its kernel.
- `Vsync::tick_all` accepts a `FrameTick` from `MotionClock`, and manual controller ticks accept `Duration`. Per-controller playback rates preserve sampled local time across changes and restarts.
- Run completion and cancellation use `AnimationRunFuture` from the animation crate.

### Removed

- Public disposal through controller observer handles; retire the owning `DrivenController` to withdraw its seat and cancel its run together.

- Public manual `Vsync` controller registration and removal; use `build_on` and the resulting `DrivenController` to own its seat.
- The scheduler ticker, ticker-provider and ticker-group APIs, process-global animation time dilation and unused epoch helpers.
- Unused `CompoundAnimation`, `AnimationOperator`, the animation prelude, and scheduler convenience re-exports from the animation crate.
