### Added

- `GlobalKey::try_new`, which reports an exhausted key space as `flui_foundation::IdExhausted`
  instead of panicking; `GlobalKey::new` panics with that cause.
- `flui_foundation::AtomicIdMint<NonZeroU32>`, a mint for identities a packed handle carries in
  32 bits.

### Changed

- `GlobalKey` identities and reactive graph ids come from `flui_foundation::AtomicIdMint`, so
  both refuse after their last value instead of reusing one.
