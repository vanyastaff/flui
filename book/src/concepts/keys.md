# Keys

Keys control how the reconciler matches a new `View` against an existing `Element` when a parent
rebuilds — without one, matching falls back to position and type, which breaks state when a list
of children is reordered or filtered. This is the same problem Flutter's key system solves, and
FLUI's shape follows it closely.

The base key types — `Key`, `ValueKey`, `UniqueKey`, `ViewKey` — live in `flui-foundation` and are
re-exported through `flui-view`'s `key` module, alongside `Keyed`/`WithKey` for attaching a key to
a view. Two widget-layer key types are defined in `flui-view` itself, matching where Flutter puts
the equivalents in `widgets/framework.dart`:

- `ObjectKey` (`crates/flui-view/src/key/object_key.rs`) — keyed by a value's identity/equality,
  for reordering a list of items each backed by a distinct object.
- `GlobalKey<T>` (`crates/flui-view/src/key/global_key.rs`) — a key unique across the whole tree,
  letting you reach an `Element`'s `State` (or an associated `RenderObject`) directly from outside
  the normal parent-to-child data flow — the same escape hatch Flutter's `GlobalKey` provides, used
  sparingly for the same reason (it breaks locality).

```rust,ignore
use flui_view::key::{GlobalKey, ObjectKey};
```

*(illustrative import path — see the `key` module's own doc comment in
`crates/flui-view/src/key/mod.rs` for the authoritative re-export list.)*
