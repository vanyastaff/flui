# flui-assets

The decoded-image cache of [FLUI](https://github.com/vanyastaff/flui). `flui-widgets` resolves
`Image::asset` and `Image::network` through the one each UI runtime keeps
(`ImageCacheCapability`); an application reaches it through those widgets.

An `ImageCache` keeps completed images within a byte budget, shares one load among every
request for the same source, reads an application's asset through the host's asset bundle on the
IO lane of the caller's lanes and decodes it on their compute lane, and fetches network images
with the host's HTTP client. It starts no runtime or thread of its own.

```rust
use std::sync::Arc;

use flui_assets::{ImageCache, ImageCacheConfig, ImageSource, LoadServices};
use flui_platform::assets::{AssetBundle, AssetKey, DirectoryAssets};

let cache = ImageCache::new(ImageCacheConfig::default().budget(64 * 1024 * 1024));
let bundle = AssetBundle::new(Arc::new(DirectoryAssets::new("flui_assets")));
let logo = ImageSource::asset(AssetKey::try_new("assets/logo.png").expect("a key"), &bundle);
assert!(cache.get(&logo).is_none());
let load = cache.load(&logo, &LoadServices::new());
drop(load); // the last waiter left: the load is cancelled
```

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) states how loads are shared, cancelled, bounded
and scheduled, and the tests that pin each rule.

## License

MIT OR Apache-2.0
