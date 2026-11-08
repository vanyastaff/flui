# flui_assets documentation

The crate owns decoded-image loading and retention, not a generic asset-cache
framework.

- [User guide](GUIDE.md): facade setup, application ownership, file and network
  loads, budgets, failures and memory pressure.
- [Architecture](ARCHITECTURE.md): source identity, shared loads, lanes,
  cancellation, scheduling and synchronization.
- [Design patterns](PATTERNS.md): the ownership and capability boundaries used
  by the registry and widgets.
- [Performance and memory](PERFORMANCE.md): decoded-byte accounting, bounded
  reads/decodes and measurement guidance.

## Quick reference

```rust
use flui_assets::{AssetRegistryBuilder, LoadServices};

# async fn load() -> Result<(), flui_assets::AssetError> {
let registry = AssetRegistryBuilder::new().build();
let image = registry
    .load_image("assets/logo.png", LoadServices::new())
    .await?;
println!("decoded {}x{}", image.width(), image.height());
# Ok(())
# }
```

Build requires no capacity selection. The only completed-image store is the
registry's byte-budgeted LRU. Concurrent requests for one source share work;
errors are not cached.

## Optional features

| Feature | Description |
|---------|-------------|
| `images` | Image decoding, retention, file/network sources and load services |
| `full` | Every stable feature; currently equivalent to `images` |

Network loading needs `images` plus an HTTP client in `LoadServices`; there is
no separate `network` feature. Facade applications normally enable `flui/assets`
instead of selecting these implementation features themselves.

## Commands

```bash
cargo heli test -p flui-assets --all-features --locked
cargo heli doc -p flui-assets --all-features --locked
```

## Project information

- [Crate README](../README.md)
- [FLUI README](../../../README.md)
- [Contributing](../../../CONTRIBUTING.md)
- [ADR-0317](../../../docs/adr/ADR-0317-the-asset-registry-is-image-only-and-the-facade-opts-into-it.md)
