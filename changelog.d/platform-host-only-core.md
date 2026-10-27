### Changed

- **`flui-platform`**: a requested URI list arrives as `TransferPayload::Custom` of
  `text/uri-list` (RFC 2483, one URI per CRLF-terminated line, a local path as its `file:` URI in
  the host's spelling), and an image as `TransferPayload::Custom` of its container type with the
  encoded bytes; a drag source serves them the same way
  ([ADR-0038](/docs/adr/ADR-0038-data-transfer-architecture.md)).
- **`flui-platform`**: `TransferRequest::channel()` is `TransferRequest::pending()`, which returns
  the request with its `TransferCompleter`.

### Added

- **`flui-platform`**: `DeviceInputRegion::project`, the one projection of an input region to
  device pixels (rounded outward; an empty, non-finite or out-of-`i32` region is refused with
  `NotProjected`), which every host and the testing host use.

### Removed

- **`flui-platform`**: the host-only tables leave the contract for the core host
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §7): `OfferTable`,
  `OfferRecord`, `TransferUri`, `TransferImage`, `TransferPayload::UriList`,
  `TransferPayload::Image` and `ProjectedInputRegions`. `device_to_logical`, `logical_to_device`,
  `offset_from_coords` and `delta_offset_from_coords`, which nothing called, are deleted.
