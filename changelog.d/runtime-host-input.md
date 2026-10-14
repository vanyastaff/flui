### Added

- **One host-input boundary** (`flui-runtime`, ADR-0207): `HostInput::{Platform, Window, Frame,
  Accessibility}` is the only input a presentation accepts, admitted through
  `PresentationDispatcher::dispatch`; a frame's vsync paces motion sampling, an assistive
  request is decoded on the owner thread. `UiRuntime::input_capabilities` keeps a window's
  latest `InputCapabilities`.
- **Input sequence identity** (`flui-foundation`, `flui-interaction`): `InputSequenceId` and its
  owner-local `IdMint`, which refuses permanently once exhausted. `GestureBinding` mints one per
  contact, wheel and trackpad latch, and contactless source (`begin_contactless`, on reserved
  `DeviceId::KEYBOARD`/`ASSISTIVE`/`PROGRAMMATIC` devices); `dispatch_pointer_on` reports it in
  a `DispatchOutcome`.
- **Presets per pointer kind** (`flui-interaction`, `flui-rendering`, ADR-0212):
  `GestureProfiles` holds one preset per kind; recognizers admit a contact with its kind's
  profile and hit testing carries the pointer kind, so `HitSlop`'s minimum target follows the
  contact. Runners configure `GestureProfiles::preset(platform)`.

### Changed

- **Arena deadlines are `EventTime`s** (`flui-interaction`): `GestureArenaMember::deadline`,
  `poll_deadline`, `GestureArena::now` and `GestureBinding::next_deadline` use the event
  timeline the interaction clock reads; `MonotonicClock::epoch` and `event_epoch` place a clock
  on it. The binding's resampling window follows its clock, never the wall clock.
- **Breaking:** `PresentationDispatcher::observe`/`input` became `dispatch(HostInput)`,
  `FrameDispatcher::deliver` takes the frame's vsync, `PresentationWindow::with_gesture_preset`
  became `with_gesture_profiles`, `GestureSettingsProvider::snapshot`/`min_target` became
  `snapshot_for`/`min_target_for(kind)`, and `PipelineOwner::hit_test`,
  `RendererBinding::hit_test_in_view`, `RenderObject::hit_slop_area`, `HitTestProbe::probe`,
  `HitTestHandle::hit_test_at` and `MouseTracker::update_all_devices` take a `PointerKind`.
