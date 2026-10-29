### Added
- The window conformance kit checks that a frame request's vsync and target present lie on the event timeline (`a_frame_request_is_stamped_on_the_event_timeline`).

### Fixed
- Win32 frame requests carry the compositor's refresh: DWM reports its next vblank ahead of the reading, which the host refused as unplaceable, so every request fell back to the reading with no target present time.
