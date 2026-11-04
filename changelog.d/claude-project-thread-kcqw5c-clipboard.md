### Changed

- `flui_platform::data_transfer::DataTransferSource` is renamed `DragTransport`, and
  `NullDataTransferSource` is renamed `NullDragTransport` (also re-exported at the crate root):
  the transport carries drag and drop only (ADR-0312). `Platform::data_transfer()` and
  `RuntimeHostServices::with_data_transfer` keep their names and take or return
  `Arc<dyn DragTransport>`.

### Removed

- `DataTransferSource::clipboard_offer`, which every host answered `None`. The clipboard is read
  and written through `ClipboardHandle` as plain text; ADR-0312 §7 reserves the shape of a richer
  read, which lands with its first consumer.
