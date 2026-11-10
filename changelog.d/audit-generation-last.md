### Added

- **`flui-foundation`**: `Generation::LAST` (`0x7FFF_FFFF`), `generation::DOMAIN_BIT` and
  `Generation::try_from(NonZeroU32)`, which refuses a raw generation that sets the packed key's
  reserved top bit with `ReservedGenerationBit`
  ([ADR-0320](/docs/adr/ADR-0320-packed-keys-reserve-their-top-bit-and-synthetic-semantics-ids-set-it.md) §1).
- **`flui-semantics`**: `SemanticsIdentitySource::release`, which returns a synthetic identity's
  slot under its next generation and refuses a stale or second release.

### Changed

- **`flui-foundation`**: a packed generational key reserves its top bit for a domain tag, so a
  generation has 31 bits and a slot closes at `Generation::LAST`. `new_gen` on `GenId`
  (`RenderId`, `UiRuntimeId`, `DataTransferId`), `ElementId` and `PresentationId` takes a
  `Generation` instead of a `NonZeroU32`, and their `generation()` returns one; a raw value
  goes through `Generation::try_from`. `from_u64` and `ElementId`'s `Deserialize` refuse a
  value that sets the reserved bit.
- **`flui-semantics`**: synthetic accessibility identities (authored regions, text runs,
  announcers) are generational slot keys whose published `AccessibilityNodeId` sets the reserved
  top bit, so none aliases a render identity, instead of values below `2^32`. The pipeline
  releases a region's or a text run's identity when its key leaves the tree, and the slot
  returns under its next generation, so churn no longer spends the identity space
  ([ADR-0320](/docs/adr/ADR-0320-packed-keys-reserve-their-top-bit-and-synthetic-semantics-ids-set-it.md) §2).
