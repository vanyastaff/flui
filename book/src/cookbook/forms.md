# Forms

`examples/material_demo` includes a form section: a validated `TextField` with an inline error,
and a Submit button that only enables once the field is valid.

```bash
cargo run --example material_demo --features material
```

Source: [`examples/material_demo/tree.rs`](https://github.com/vanyastaff/flui/blob/main/examples/material_demo/tree.rs).

For the headless test pattern that exercises this without a window, see
[`docs/testing.md`](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) — the form and
async sections of this same demo each have facade-only headless tests, and
[Testing](testing.md) walks through the equivalent for the counter example.
