### Changed

- A focusable control costs less to mount (a `RawButton` over a `Text`:
  `crates/flui-widgets/perf/baseline/raw_button_over_text.toml`). A focus node keeps its key
  handler with its other rarely set registrations, a focus host keeps its key-handler and
  traversal registrations behind one box, a focus configuration keeps its handlers and traversal
  links behind one shared pointer, and the bindings a focus host records on its node live in the
  state its listener already shares instead of an allocation of their own. A node, `Focus` or
  `Shortcuts` given a key handler pays for those boxes on its first one.
- `SubtreeAnchor` is owner-thread state: it holds its `RenderId` in an `Rc<Cell<_>>` and is no
  longer `Send` or `Sync`.
- A notification channel's listener storage is smaller.

### Fixed

- A `Focus` whose caller-owned node was adopted elsewhere clears what it recorded on the node
  when it is disposed: the node no longer keeps the disposed widget's focus callbacks alive, and
  a `Shortcuts` above no longer resolves through the disposed widget's `Actions`.
