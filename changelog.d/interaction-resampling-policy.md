### Added
- `AppConfig::with_pointer_resampling` opts a window presentation into frame-aligned measured pointer interpolation. The policy is disabled by default and cannot change during an active contact.

### Fixed
- Opted-in presentations map event admission and frame sampling onto their owner clock, preserve pending sample delivery across frames, and keep sibling input policies independent.
