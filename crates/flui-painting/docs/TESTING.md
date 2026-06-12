# Painting test harness (`flui_painting::testing`)

Record [`DisplayList`](../src/display_list/mod.rs) commands without
`Canvas::new()` / `finish()` boilerplate, then inspect command count, bounds,
and diagnostics.

Use this when testing **paint recording in isolation** (a single render object's
`paint` method, a custom painter, or display-list utilities). For full render-tree
integration, prefer [`flui-rendering::testing`](../../flui-rendering/docs/TESTING.md).

## Enabling

| Consumer | How to enable |
|----------|----------------|
| This crate's own tests | `cfg(test)` |
| Downstream crates | `flui-painting = { features = ["testing"] }` |

```bash
cargo test -p flui-painting
cargo test -p flui-painting --test display_list_unit
```

## Design

```text
record(|canvas| { … })  →  DisplayList  →  command_count / bounds / diagnostics
```

The harness wraps the canonical record-now pattern: one closure, one finished list.

## Core API

All functions live in [`testing/mod.rs`](../src/testing/mod.rs).

| Function | Returns | Purpose |
|----------|---------|---------|
| `record(f)` | `DisplayList` | `Canvas::new()` → run `f` → `finish()` |
| `command_count(list)` | `usize` | Number of recorded commands |
| `bounds(list)` | `Rect` | Record-time bounds |
| `diagnostics(list)` | `DiagnosticsNode` | `Diagnosticable` snapshot |
| `dump(list)` | `String` | Indented printable dump |

## Examples

### Record a filled rect

```rust
use flui_painting::testing::{record, command_count, bounds};
use flui_painting::Paint;
use flui_types::{Rect, geometry::px, styling::Color};

let list = record(|canvas| {
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
        &Paint::fill(Color::RED),
    );
});

assert_eq!(command_count(&list), 1);
assert_eq!(
    bounds(&list),
    Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
);
```

### Structured diagnostics (no substring matching)

```rust
use flui_painting::testing::{record, diagnostics};

let list = record(|canvas| { /* draw */ });
let node = diagnostics(&list);
assert_eq!(node.name(), Some("DisplayList"));
assert_eq!(node.get_property("commands"), Some("1"));
```

### Empty recording

```rust
let list = record(|_canvas| {});
assert_eq!(command_count(&list), 0);
```

## See also

- Crate architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md)
- Render-object harness (end-to-end paint): [`flui-rendering/docs/TESTING.md`](../../flui-rendering/docs/TESTING.md)
- Workspace overview: [`docs/testing.md`](../../../docs/testing.md)
