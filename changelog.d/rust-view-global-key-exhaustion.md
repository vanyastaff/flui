### Changed

- `GlobalKey::id()` returns the typed `GlobalKeyId` over a private `NonZeroU64`; arbitrary `u64`
  values no longer type-check as global-key identities. Identity allocation uses the shared
  `AtomicIdMint` policy.

### Fixed

- Refuse GlobalKey identity allocation permanently after issuing `u64::MAX`, preventing wraparound from issuing zero and reusing earlier keys.
