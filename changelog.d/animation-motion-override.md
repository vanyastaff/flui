### Changed

- **The application motion setting is `MotionOverride`** (`flui-animation`, `flui-runtime`,
  `flui-app`, `flui-testing`): `MotionPreference` (FollowSystem, Reduce, Full) is renamed
  `MotionOverride` so it no longer shares a name with `flui_platform::MotionPreference`, the
  host's observation. `MotionClock::set_preference` is `set_override`; `set_motion_preference`,
  `with_motion_preference` and `AppConfig::motion_preference` become `set_motion_override`,
  `with_motion_override` and `motion_override` (ADR-0218).
