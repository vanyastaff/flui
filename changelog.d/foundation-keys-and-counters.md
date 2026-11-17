### Removed

- `flui::foundation::Key` and `KeyRef`. A `Key` from a string's hash, from a counter and from a
  number shared one value space, so `Key::from_u64(7)` matched the seventh `Key::new()`; a view
  is keyed by `ValueKey` (a constant is `ValueKey::new("header")`) or `UniqueKey`.
- `flui::foundation::GenerationGate` and `ResourceGeneration`, which nothing used: an asset load
  is matched to its request by its load ticket.

### Changed

- `UniqueKey::new` mints from an `AtomicIdMint`.
