### Added

- Input recordings name the input sequence of each pointer row and arena verdict, the surface an
  input addressed (`RecordedSurface`), and each arena member by its number in the recording
  (`RecordedMember`); a recording header holds every window's per-kind gesture values and the
  system-preference revision in effect, and `HeadlessHost::replay` restores both first.
- `UiRuntime::handle_input_on` and `PresentationDispatcher::dispatch_on` route platform input to a
  surface embedded in a window; `HeadlessHost::embed_surface` and `HeadlessHost::dispatch_on`
  drive it headlessly.
- `GestureContest::sequence`, `GestureSettings::values` / `try_from_values` (`GestureValues`),
  `GestureProfiles::iter` / `with_kind` and `GestureBinding::replace_default_profiles`.
- `flui-sdk` re-exports arena observation (`GestureObserver`, `ObserverHandle`,
  `GestureObservation` and the contest types), `GestureProfiles`, `GestureSettings`,
  `SystemPreferences`, `TargetPlatform`, `HapticFeedback` and the input-region sink and values.
- `IdMint` issues raw `NonZeroU64` identities under a ceiling and below a reservation, for
  identity types defined above `flui-foundation`.

### Changed

- `flui-runtime`'s test-hook feature is `testing`, as in every other crate (was `test-support`).
- `DecisionRow::member` is a `RecordedMember` number; its type name moved to `member_type`.
- `InputTaps::first_failure` returns a `TapFailure` (site and message) instead of a string.
- `RecordingHeader` holds `preferences` (a `PreferencesRow`) and per-window gesture values in
  place of a preset name and a bare revision; `RecordingHeader::with_window` takes a
  `RecordedWindow`.
- `InputSurface` is a closed enum.
- `ContactRegistry::with_click_settings` is gone; `set_click_settings` sets the thresholds at start
  and after a change.

### Fixed

- Writing a recording whose vocabulary grew a variant the format does not mirror fails with
  `RecordingError::Unmirrored` instead of panicking.
- A pointer device registry started near the top of the identity range no longer hands a reserved
  contactless device id to hardware.
