### Removed

- **`flui_view::GlobalKeyId`**: a `pub type GlobalKeyId = u64` with no caller anywhere in the
  workspace — a public alias that invited naming a raw `u64` a key identity. A `GlobalKey`'s own
  id stays private to the key and its registry, minted by the key's counter (audit T11b; a
  newtype would have minted an identity no one reads).