# ADR-0385: flui-assets is the decoded-image cache

- **Status:** Accepted (2026-11-19)
- **Date:** 2026-11-19
- **Supersedes:** [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md),
  [ADR-0107](ADR-0107-asset-byte-sources-and-decoding.md),
  [ADR-0120](ADR-0120-typed-asset-cache-retention.md) and
  [ADR-0121](ADR-0121-owned-asset-key-names.md), whole
- **Related:** [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md),
  [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md),
  [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md)

## Context and problem statement

`flui-assets` held two mechanisms. The first is the decoded-image cache that `flui-widgets`
resolves `Image::asset` and `Image::network` through: a byte-budgeted LRU of decoded images, one
load shared by every request for a source, reads on the caller's IO lane and decodes on its
compute lane (ADR-0250, ADR-0279, ADR-0280). The second was a generic typed asset framework:
an `Asset` trait with its own key, data and error types, a per-type `AssetCache` on `moka`,
`AssetHandle` and `WeakAssetHandle` with sealed `…Core`/`…Ext` traits, `AssetKey`,
`AssetMetadata` with audio and video fields, `LoadState`, `CacheStats`, `FontAsset`,
`ImageAsset`, `BytesFileLoader` and a type-state `AssetRegistryBuilder`. ADR-0105, ADR-0107,
ADR-0120 and ADR-0121 decided its validation, byte sources, retention and key ownership.

Nothing outside the crate's own tests and documents used the second mechanism. Fonts reach the
text system through `flui_app::register_font`. `ImageAsset` read a file whole through
`tokio::fs::read`, without the encoded-size limit the decoded-image path checks, so the two
paths to one image disagreed on what they accept. The typed cache told the built-in font
type apart from every other type by `TypeId` to give it different coalescing. The framework
brought `moka` and Tokio's file system and runtime into a substrate crate.

## Decision drivers

- A public contract has a caller (AGENTS.md, "a new `pub` item that no production path
  reaches").
- One path to an image: the limits, the coalescing and the cancellation an image load has are
  the same however it is requested.
- Flutter has no generic asset type: `AssetBundle` reads bytes by key and `ImageCache` keeps
  decoded images; a font is loaded by `FontLoader` from bundle bytes.
- A substrate crate depends on what its one job needs.

## Considered options

1. **Keep both mechanisms**, and route `ImageAsset` through the decoded-image limits.
2. **Delete the generic framework; the crate is the decoded-image cache.**

## Decision

Chosen option: 2. `flui-assets` exports one cache and what a load needs:

```rust
pub struct ImageCache;                 // new(ImageCacheConfig), Default, ReclaimMemory
pub struct ImageCacheConfig;           // Default; budget(bytes), decode_threads(n)
pub struct ImageSource;                // an asset key with its bundle, or a URL with its client (ADR-0386, ADR-0384 §6)
pub struct LoadServices;               // the caller's lanes
pub struct ImageLoad;                  // Future<Output = Result<Image, ImageLoadError>>
```

`ImageCache` is the former `AssetRegistry` without its typed caches, and `ImageCacheConfig`
replaces the builder: a cache has no required setting, so it needs no type state. The crate has
no public modules and does not re-export `flui_painting::paint::Image`. The `images` feature is
gone: the image path is the crate. `moka`, Tokio's `fs`, `io-util` and `rt` features, and every
type listed in the context are deleted with their tests.

The decoded-image rules stay where they are: ADR-0250 for what the cache keeps, ADR-0279 for
the lanes a load runs on, ADR-0280 for network fetches. Reading an asset from the
application's bundle is ADR-0384's ([ADR-0384](ADR-0384-an-application-reads-its-assets-by-key-from-the-bundle-its-host-provides.md) §6).

### Confirmation

- `crates/flui-assets/src/lib.rs` exports only the types above and the error types; `rg
  'AssetRegistry|AssetCache|AssetHandle|AssetKey|FontAsset|ImageAsset|BytesFileLoader'` over
  `crates/`, `packages/`, `src/` and `docs/` outside superseded records finds no user.
- `Cargo.lock` has no `moka`.
- The decoded-image suites (`image_load`, `network_load`, `pixel_limit`,
  `image_decode_scheduling`) run without a feature flag.

## Pros and cons of the options

### 1. Keep both mechanisms

- Good, because an application could cache its own asset types through the crate.
- Bad, because no application or framework code does, and each typed asset type repeats a
  limit, a coalescing rule and a cancellation rule the image path already holds.
- Bad, because it keeps `moka` and Tokio's runtime in a substrate crate for no caller.

### 2. Delete the generic framework

- Good, because an image has one path, with one set of limits.
- Good, because the crate's API is what `flui-widgets` uses, and its dependencies are what that
  path needs.
- Bad, because an application that wants a typed cache of its own data writes it itself; it
  can hold the bytes it reads in any cache it chooses.
