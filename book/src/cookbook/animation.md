# Animation

`examples/animated_box_app` drives FLUI's animation engine through the real render pipeline on
GPU.

```bash
cargo run --example animated_box_app
```

Source: [`examples/animated_box_app.rs`](https://github.com/vanyastaff/flui/blob/main/examples/animated_box_app.rs).

`flui-widgets`'s `animated/` and `transitions/` modules
([`crates/flui-widgets/src/animated/`](https://github.com/vanyastaff/flui/tree/main/crates/flui-widgets/src/animated),
[`crates/flui-widgets/src/transitions/`](https://github.com/vanyastaff/flui/tree/main/crates/flui-widgets/src/transitions))
are the widget-level entry points most application code reaches for; `animated_box_app` exercises
the engine underneath them directly.
