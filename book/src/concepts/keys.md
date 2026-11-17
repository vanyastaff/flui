# Keys

Keys control how the reconciler matches a new `View` against an existing `Element` when a parent
rebuilds — without one, matching falls back to position and type, which breaks state when a list
of children is reordered or filtered.

The base key types — `Key`, `ValueKey`, `UniqueKey`, `ViewKey` — live in `flui-foundation` and are
re-exported through `flui-view`'s `key` module. A view holds its key in a field marked
`#[view(key)]`, which the `StatelessView` and `StatefulView` derives read: the field is a key, an
`Option` of one, or an `Rc<dyn ViewKey>` or `Box<dyn ViewKey>`.

```rust,ignore
#[derive(Clone, StatelessView)]
struct TodoRow {
    #[view(key)]
    key: ValueKey<u64>,
    title: String,
}
```

Two widget-layer key types are defined in `flui-view` itself:

- `ObjectKey` (`crates/flui-view/src/key/object_key.rs`) — keyed by a value's identity/equality,
  for reordering a list of items each backed by a distinct object.
- `GlobalKey<T>` (`crates/flui-view/src/key/global_key.rs`) — a key unique across the whole tree,
  letting you reach an `Element`'s `State` (or an associated `RenderObject`) directly from outside
  the normal parent-to-child data flow. Use it sparingly: it breaks locality.

```rust,ignore
use flui_view::key::{GlobalKey, ObjectKey};
```

*(illustrative import path — see the `key` module's own doc comment in
`crates/flui-view/src/key/mod.rs` for the authoritative re-export list.)*
