# flui-semantics

**The accessibility tree — the fifth tree in FLUI's five-tree architecture**
(View → Element → Render → Layer → **Semantics**).

`flui-semantics` carries the information assistive technologies need — screen
readers (VoiceOver, TalkBack, NVDA, JAWS), switch control, voice control,
braille displays — mirroring Flutter's semantics protocol:

| FLUI | Flutter |
|------|---------|
| `SemanticsNode` | `SemanticsNode` |
| `SemanticsConfiguration` | `SemanticsConfiguration` |
| `SemanticsOwner` | `SemanticsOwner` |
| `SemanticsAction` / `SemanticsEvent` | `SemanticsAction` / semantics events |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## How it fits the pipeline

```text
RenderObject (flui-rendering)
    │  assembleSemanticsNode() during the paint phase
    ▼
SemanticsNode (this crate)  —  SemanticsTree (slab storage, 1-based SemanticsId)
    │  flush() batches dirty nodes into a SemanticsTreeUpdate
    ▼
Platform accessibility API (via flui-platform backends)
```

- `SemanticsTree` implements the generic `flui-tree` traits (`TreeRead`,
  `TreeNav`, `TreeWrite`), so tree walks share the workspace's cycle-guarded
  iterators.
- `add_child` enforces cycle rejection at the public API; the `Ancestors`
  iterator adds defence-in-depth bounding against corrupted parent pointers.
- Labels/hints use `SmolStr` (O(1) clone), children/actions use `SmallVec`,
  lookups use `FxHashMap`.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-semantics --open`. Architecture context lives in
[`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
