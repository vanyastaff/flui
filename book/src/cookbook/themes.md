# Themes

Both widget-catalog demos set up a theme explicitly before building their tree:

```bash
cargo run --example material_demo --features material    # ThemeData / ColorScheme
cargo run --example cupertino_demo --features cupertino   # CupertinoTheme
```

Source: [`examples/material_demo/tree.rs`](https://github.com/vanyastaff/flui/blob/main/examples/material_demo/tree.rs),
[`examples/cupertino_demo/tree.rs`](https://github.com/vanyastaff/flui/blob/main/examples/cupertino_demo/tree.rs).

The minimal shape — wrap the root view in a theme, then call `run_app` — is also what
`examples/counter.rs` does with `Theme::new(ThemeData::light(), ...)`; see the
[Flutter → FLUI mapping](../mapping.md) table's `MaterialApp` row for why that's the pattern rather
than a single `MaterialApp`-style widget.
