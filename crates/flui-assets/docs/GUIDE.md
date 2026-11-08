# User guide

## Enable image assets

Most applications consume the registry through the facade:

```toml
[dependencies]
flui = { path = "…", features = ["assets"] }
```

This exposes `flui::assets`, enables `flui-assets/images` and turns on
`flui-widgets/asset-images`. It does not change `flui::prelude`. Without the
feature, `flui::assets` is absent rather than an empty compatibility module.

Direct users enable `flui-assets/images`. Network loading is part of that same
feature and uses a caller-provided HTTP client; no `network` feature exists.

## Own one registry per asset domain

```rust
use std::sync::Arc;
use flui::assets::AssetRegistryBuilder;
use flui::widgets::Image;

let registry = Arc::new(AssetRegistryBuilder::new().build());
let logo = Image::asset(Arc::clone(&registry), "assets/logo.png");
let avatar = Image::network(
    Arc::clone(&registry),
    "https://example.com/avatar.png",
);
```

The application decides the registry's lifetime and which widgets share it.
Different registries retain different images and pending loads even for equal
paths. The framework has no process-wide registry and the host does not acquire
one implicitly.

`AssetRegistryBuilder::new().build()` is complete without a capacity call. Its
only retention setting is a decoded-byte budget; decode concurrency is the
other optional setting.

## Load directly

```rust
use flui_assets::{AssetRegistryBuilder, LoadServices};

# async fn example() -> Result<(), flui_assets::AssetError> {
let registry = AssetRegistryBuilder::new().build();
let image = registry
    .load_image("assets/logo.png", LoadServices::new())
    .await?;
assert_eq!(image.data().len(), image.width() as usize * image.height() as usize * 4);
# Ok(())
# }
```

`load_network_image(url, services)` has the same cache, sharing and cancellation
behavior. Its `services` must include an HTTP client or the future resolves to
`AssetError::HttpUnavailable` without sending a request.

A load reads or fetches on the services' IO lane and decodes on their compute
lane. With `LoadServices::new()` there are no lanes: work happens where the
returned `ImageLoad` is polled. The registry never starts an ambient runtime or
a thread of its own.

## Probe from a frame

`cached_image(&ImageSource)` returns a completed image synchronously and marks
it most recently used. It performs no file, network or decode work. Asset
sources use `ImageSource::Asset(path)`. A network source includes the fetching
client's authority; construct it through the same client rather than assuming a
URL alone identifies its bytes.

The widget providers perform this probe before subscribing to a load. This
keeps build/layout/paint synchronous while IO and decoding remain at the loading
edge.

## Choose a decoded-byte budget

```rust
use flui_assets::AssetRegistryBuilder;

let registry = AssetRegistryBuilder::new()
    .with_decoded_image_budget(128 * 1024 * 1024)
    .build();
```

The budget counts `width * height * 4` for each retained image. The default is
256 MiB, and 64 MiB on Android, iOS and the web. Once the total exceeds the
budget, least recently used images go first. There is no time-based expiration.

An image larger than the budget is delivered to its waiters but not retained.
A zero budget likewise disables completed-image retention without disabling
load sharing. Returned `Image` values own their pixels independently of later
cache eviction.

`with_decode_threads(NonZeroUsize)` bounds concurrent decodes. The default is
half the available cores, capped at four.

## Sharing, cancellation and recovery

A request for a source already loading joins that load. Dropping one `ImageLoad`
leaves work alive for other waiters. Dropping the last waiter cancels the ticket:
an HTTP request is dropped, a chunked file read stops at its next chunk, and a
decode that has not started does not run.

If lanes or the polling request drop a load without an outcome, another waiter
can take it over using its own lanes and HTTP client. Ticket identity prevents
an old completion from overwriting a newer load for the same source.

Errors are not retained. Missing files, malformed bytes, transport failures and
non-2xx responses can therefore be followed by a successful retry. A panicking
decoder fails that load rather than unwinding through a background lane.

## Memory pressure

The registry implements `ReclaimMemory`. Moderate pressure trims completed
images to half their configured budget; critical pressure clears them. Loads in
flight continue under their normal subscriber rules. An application that wants
this behavior registers its application-owned registry with
`AppConfig::with_memory_reclaim`; the host still does not own the registry.

Cache reclamation does not revoke images already returned to widgets or direct
callers. GPU texture reclamation is a separate renderer responsibility.

## Troubleshooting

- **An import does not resolve:** enable facade `assets` or crate `images` and
  import the registry explicitly; it is not in the prelude.
- **A network load returns `HttpUnavailable`:** supply the host's HTTP client in
  `LoadServices`.
- **A warm source reloads:** check that both requests use the same registry and
  source identity, and that the decoded-byte budget can retain it.
- **Memory remains high after eviction:** account for images held by callers,
  encoded bytes in admitted loads, decoder allocations and GPU textures.
- **Work makes no progress without lanes:** poll the returned `ImageLoad`; no
  hidden executor drives it.

## Next steps

- [Architecture](ARCHITECTURE.md)
- [Design patterns](PATTERNS.md)
- [Performance and memory](PERFORMANCE.md)
