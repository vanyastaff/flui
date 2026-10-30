### Changed

- `PrimarySelectionBackend::claim` returns the replaced claim's `SelectionText` with the new claim's
  number, and `release` returns the released one: the handle drops them once the new claim is
  guarded, and keeps them while unwinding.

### Fixed

- On a multi-seat Wayland compositor, copying on one seat no longer destroys the clipboard source
  another seat still reads.
- The Wayland clipboard no longer sends `wl_data_offer.destroy` to a `wl_data_device_manager`
  below version 3.
- A primary-selection claim dropped while unwinding keeps its captures instead of dropping them
  into a second panic, and a claim whose predecessor panics as it drops is withdrawn rather than
  left current with no guard.
