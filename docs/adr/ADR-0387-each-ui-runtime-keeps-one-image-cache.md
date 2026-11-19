# ADR-0387: Each UI runtime keeps one image cache

- **Status:** Accepted (2026-11-19)
- **Date:** 2026-11-19
- **Supersedes:** [ADR-0118](ADR-0118-network-image-registry-identity.md) for the widget's key;
  [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md) §4 for the key of a
  built-in provider (the cache's weak allocation identity) and §5 for who owns the cache and
  registers it for memory pressure, with its Consequences line on an unregistered cache
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (capabilities, per-runtime provider
  state), [ADR-0237](ADR-0237-window-state-and-host-notices.md) decision 9 (memory pressure),
  [ADR-0385](ADR-0385-flui-assets-is-the-decoded-image-cache.md),
  [ADR-0386](ADR-0386-an-image-source-carries-its-client-and-a-failed-load-names-its-kind.md)

## Context and problem statement

`Image::asset(cache, path)` and `Image::network(cache, url)` took an `Arc<ImageCache>` the
application made and passed to every image. Nothing made one for it: an application that wanted
images shared across its widgets had to keep a cache in some state of its own and thread it down
the tree, and one that built a cache per widget kept a decode per widget. Under memory pressure
the cache gave its pixels back only if the application also registered it with
`AppConfig::memory_reclaim`, and one that forgot kept them (ADR-0250, Consequences). Because a
key had to keep two caches apart, `ImageCacheKey` held the cache's weak allocation identity.

Flutter keeps one `ImageCache` per binding (`PaintingBinding.instance.imageCache`), and
`Image.asset(name)` and `Image.network(url)` name only their source. FLUI's binding is the UI
runtime (ADR-0027), so its counterpart is one cache per runtime.

## Decision drivers

- An application writes `Image::asset(key)`, as in Flutter, with no cache to make or pass.
- Every image of one runtime shares decodes and pixels; two runtimes share nothing (ADR-0027,
  ADR-0250 §1).
- The framework gives back the memory it keeps without an application registering anything.
- Every host behaves the same, the headless one included.
- Services a widget reaches go through the capability seam (ADR-0152), which an application can
  replace.

## Considered options

1. A capability per runtime: `ImageCacheCapability`, whose built-in provider keeps the cache as
   runtime-local state the runtime trims under memory pressure.
2. A field of `UiRuntime`, reached through a new `LifecycleContext` method.
3. One cache per application in `flui-app`, registered among the host's memory receivers.
4. Keep the argument (status quo).

## Decision

Option 1.

1. **The cache is a capability.** `flui_assets::ImageCacheCapability` (`Handle = Arc<ImageCache>`,
   name `image-cache`) lives beside the cache it hands out (ADR-0152 §1).
   `ImageCacheCapability::per_runtime` is the built-in provider: it keeps one cache per UI
   runtime, made with `ImageCacheConfig::default()` at the runtime's first acquisition, so a
   runtime that shows no asset or network image keeps none. The UI runtime's built-ins
   (`flui_runtime::capability::BuiltIns`) register it in every window system with
   `flui-runtime`'s `images` feature, which `flui-app` and the headless host of `flui-testing`
   forward from their own, so a test acquires the cache exactly when a run does; the facade's
   `images` turns them on with `flui-widgets`' loaders and publishes `flui::assets`. An
   application that wants another budget or one cache across its runtimes provides its own with
   `Application::capability::<ImageCacheCapability>(provider)`.
2. **A runtime trims the caches its providers keep.** `ProviderContext::runtime_cache::<T>()`
   (`RuntimeLocals::get_or_default_reclaimed`) is `runtime_local::<T>()` for a `T` that is also
   `ReclaimMemory`: the runtime lists the slot once, and on `HostNotice::MemoryPressure` trims
   every listed slot after its text caches and before any subscriber hears the notice, each trim
   contained and the first panic resumed after the rest. A retired runtime lists nothing. The
   host's own receivers (`AppConfig::memory_reclaim`) are trimmed once per notice before every
   runtime's, as before.
3. **`Image` acquires it with its other services.** In `init_state` it acquires
   `ImageCacheCapability` beside `BackgroundCapability` and `HttpCapability` and hands it to its
   provider in `ImageServices` (`image_cache()`, `with_image_cache`). `AssetImage::new(path)` and
   `NetworkImage::new(url)` take no cache. Without one, a load answers
   `ImageProviderError::ImageCacheUnavailable { image }` and reads nothing, as a network image
   without a client answers `HttpUnavailable`.
4. **A built-in key is its source.** `ImageCacheKey` for `AssetImage` is the path and for
   `NetworkImage` the URL. The cache an `Image` acquired does not change while it is mounted, so
   the key need not name it, as it already leaves out the client (ADR-0280 §6).

```rust
pub enum ImageCacheCapability {}
impl Capability for ImageCacheCapability {
    type Handle = Arc<ImageCache>;
    const NAME: &'static str = "image-cache";
}
impl ImageCacheCapability {
    pub fn per_runtime(ctx: &ProviderContext<'_>) -> Result<Arc<ImageCache>, Unsupported>;
}

impl ProviderContext<'_> {
    pub fn runtime_cache<T: Default + ReclaimMemory + 'static>(&self) -> Rc<T>;
}
impl RuntimeLocals {
    pub fn get_or_default_reclaimed<T: Default + ReclaimMemory + 'static>(&self) -> Rc<T>;
    pub fn reclaim(&self, level: MemoryPressureLevel, failure: &mut FirstPanic);
}

impl Image {
    pub fn asset(key: impl Into<String>) -> Self;   // read through the asset bundle (ADR-0384 §6)
    pub fn network(url: impl Into<String>) -> Self;
}
impl ImageServices {
    pub fn with_image_cache(self, cache: Arc<ImageCache>) -> Self;
    pub fn image_cache(&self) -> Option<&Arc<ImageCache>>;
}
```

### Consequences

- An application turns on `images` and writes `Image::asset(key)`; a network image also needs
  `http`. It registers nothing for memory pressure.
- `MemoryImage` and `FileImage` still decode per widget: they take no cache, so two widgets
  with equal bytes decode twice.
- A custom `ImageProvider` reaches the runtime's cache through `ImageServices::image_cache`.

### Confirmation

- `asset_images_share_their_runtimes_cache_until_memory_pressure` (`flui-widgets`): an
  `Image::asset` decodes into the cache its runtime hands out, a second acquisition in that
  runtime gets the same cache and another runtime its own, and a critical memory-pressure notice
  empties it.
- `reclaimed_runtime_locals_are_trimmed_until_retirement` (`flui-platform`): a slot asked for
  as a cache is trimmed once per notice however often it was asked for, one made plain first is
  listed when asked for as a cache, a panicking trim leaves the later ones trimmed, and a retired
  runtime trims nothing.
- `network_image_loads_with_the_hosts_http_client` and
  `an_image_missing_a_service_answers_which_one` (`flui-widgets`).

## Pros and cons of the options

### 1. A capability per runtime

- Good, because it is the seam every other service a widget reaches goes through, so an
  application replaces it the same way and a host that lacks it answers `Unsupported`.
- Good, because the trim belongs to the slot, so a later per-runtime cache (a font or a glyph
  cache a provider keeps) gives memory back without a registration of its own.
- Bad, because it adds a method to `ProviderContext` and two to `RuntimeLocals`.

### 2. A field of `UiRuntime`

- Good, because no capability is acquired.
- Bad, because `flui-runtime` would depend on `flui-assets` and image decoding for every
  application, images or not, and an application could not replace the cache.

### 3. One cache per application

- Good, because two windows on two runtimes would share pixels.
- Bad, because a cache touched by two runtimes is state the process owns rather than a runtime
  (ADR-0027), and the headless host would need its own copy of the registration.

### 4. Keep the argument

- Good, because nothing changes.
- Bad, because every application repeats the plumbing, and the one that forgets the registration
  keeps its pixels under pressure.
