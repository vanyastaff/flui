### Changed

- **`flui-platform`**: a requested URI list arrives as `TransferPayload::Custom` of
  `text/uri-list` (RFC 2483, one URI per CRLF-terminated line, a local path as its `file:` URI),
  and an image as `TransferPayload::Custom` of its container type with the encoded bytes; a
  drag source serves them the same way. `Key::Character` carries its `String`. A backend makes a
  pending request with `TransferRequest::pending` from a `flui_foundation::claim_slot` handle
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §7).
- **`flui-native`, `flui-testing`**: `MockWindow::input_region_updates` and
  `HeadlessWindow::input_region_updates` report each region as its kind and device rectangle.

### Removed

- **`flui-platform`**: the host-only tables leave the contract for the core host:
  `OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferRequest::channel`, `TransferUri`,
  `TransferImage`, `TransferPayload::UriList`, `TransferPayload::Image`, `DeviceInputRegion` and
  `ProjectedInputRegions`. `KeyText`, `device_to_logical`, `logical_to_device`,
  `offset_from_coords` and `delta_offset_from_coords`, which nothing called, are deleted.
