# flui_assets

Application-owned decoded-image loading for FLUI. One registry shares in-flight
file and network loads and retains completed RGBA8 images in a byte-budgeted
LRU. It starts no runtime or thread of its own.

## Use through the facade

```toml
[dependencies]
flui = { path = "…", features = ["assets"] }
```

```rust
use std::sync::Arc;
use flui::assets::AssetRegistryBuilder;
use flui::widgets::Image;

let registry = Arc::new(AssetRegistryBuilder::new().build());
let image = Image::asset(Arc::clone(&registry), "assets/logo.png");
let remote = Image::network(
    Arc::clone(&registry),
    "https://example.com/logo.png",
);
```

The `assets` feature exposes `flui::assets` and enables the asset/network image
widget providers. Nothing is added to `flui::prelude`; without the feature the
module and those providers are absent. An application owns and shares the
registry. The host does not create a process-global registry.

Direct users can depend on `flui-assets` with its `images` feature and call
`load_image` or `load_network_image`. Both return an `ImageLoad` future;
`cached_image` is the synchronous completed-image probe for a frame.

## Retention and execution

```rust
use std::num::NonZeroUsize;
use flui_assets::AssetRegistryBuilder;

let registry = AssetRegistryBuilder::new()
    .with_decoded_image_budget(64 * 1024 * 1024)
    .with_decode_threads(NonZeroUsize::MIN)
    .build();
```

- The decoded-image budget counts `width * height * 4` bytes, not encoded file
  size. Least recently used images are evicted first. An over-budget image is
  returned but not retained; zero disables completed-image retention.
- The default budget is 256 MiB, and 64 MiB on Android, iOS and the web.
- Requests for one source share a load. Dropping one waiter leaves the others
  running; dropping the last cancels file reading or the HTTP request. An
  unfinished load can be taken over by a remaining waiter on its own services.
- `LoadServices` carries the caller's IO/compute lanes and optional HTTP client.
  Files are read on the IO lane and decoded on the compute lane. With no lanes,
  work runs where the `ImageLoad` is polled.
- Network sources include the HTTP client's authority in their identity, so two
  clients serving different bytes for one URL cannot share the wrong image.
- Moderate memory pressure trims retained pixels to half the budget; critical
  pressure clears them. Images already handed to callers remain valid.

Encoded files and HTTP bodies are capped at 256 MiB. The decoder checks its
header and allocation against a 256 MiB RGBA8 limit (128 MiB on Android, iOS
and the web) before producing pixels.

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `images` | Decoded file/network images, `ImageLoad`, `ImageSource`, `LoadServices` | No |
| `full` | Every stable feature; currently `images` | No |

There is no `network` feature. HTTP is a capability supplied through
`LoadServices`; this crate links no HTTP implementation.

## Errors

Failures are typed `AssetError` values and are not retained. A later request
can therefore recover after a missing file, malformed image, failed request or
non-2xx response. A network request without a client reports `HttpUnavailable`;
lanes shutting down report `LanesRefused`.

## Documentation

- [User guide](docs/GUIDE.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Design patterns](docs/PATTERNS.md)
- [Performance and memory](docs/PERFORMANCE.md)

## Validation

```bash
cargo heli test -p flui-assets --all-features --locked
cargo heli test -p flui-assets --all-features --doc --locked
```

## License

Licensed under either of [Apache License 2.0](../../LICENSE-APACHE) or the
[MIT license](../../LICENSE), at your option.
