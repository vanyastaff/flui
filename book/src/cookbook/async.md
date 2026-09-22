# Async

`examples/material_demo` includes a scheduler-driven simulated fetch: loading state, an
error-with-Retry path, and cancellation when the route is popped mid-fetch.

```bash
cargo run --example material_demo --features material
```

Source: [`examples/material_demo/tree.rs`](https://github.com/vanyastaff/flui/blob/main/examples/material_demo/tree.rs).

This is scheduler-driven rather than `tokio`-driven — FLUI's frame scheduler
(`flui-scheduler`) is what actually paces the simulated fetch, which is the pattern to follow for
async work that needs to interact with the frame/rebuild cycle rather than run detached from it.
