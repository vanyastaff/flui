# ADR-0384: An application reads its assets by key from the bundle its host provides

- **Status:** Accepted (2026-11-19)
- **Date:** 2026-11-19
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (the seam the bundle travels
  through), [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md) (the capability this one is
  shaped like, and the fetch the Web reader uses), [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md)
  (an image load reads on its caller's IO lane), [ADR-0097](ADR-0097-no-process-global-state-gate.md)
  (no process-global state, so no `static` root bundle), [ADR-0243](ADR-0243-one-testing-kit-per-layer.md)
  (the scripted bundle lives in its contract's kit), [ADR-0082](ADR-0082-platform-api-contract-crate.md)
  (the platform readers live in `flui-native` and only `flui-app` reaches them)

## Context and problem statement

An application ships files with itself: images, fonts, data. `flui create` writes
`[assets] directories = ["assets"]` into every new project's `flui.toml`, but nothing reads that
section: `flui build` puts no file into any package, and `ImageSource::Asset` in `flui-assets`
opens its path with `std::fs` relative to the process's current directory
(`crates/flui-assets/src/registry/images.rs`, `read_in_chunks`). So on Android (an APK), iOS (an
application bundle) and the Web (a page) `Image::asset` cannot read an application's own image,
and on the desktop it reads one only when the process happens to start in the project directory.
Flutter's answer is `AssetBundle`: the tool copies the declared files into the platform's
package, and `rootBundle.load(key)` reads them back by the key they were declared under, through
the platform's own reader (`AAssetManager`, the main bundle, a URL relative to the page, a
directory beside the executable). The question is where FLUI's bundle lives, what names an
asset, where each package keeps the files and how a test reads assets without a package.

## Decision drivers

- One name per asset on every platform: the key an author writes in code is the path the project
  declares, whatever package the build makes.
- The platform reads its own package (`AAssetManager`, `NSBundle`, the page's origin), so nothing
  is unpacked to disk at start.
- No process-global state (ADR-0097): Flutter's `rootBundle` is a top-level value, and here the
  composition root provides the bundle to each UI runtime, as it provides HTTP (ADR-0280).
- A read runs on the caller's IO lane and needs no async runtime of the caller's (ADR-0279,
  ADR-0047).
- A key is untrusted input once it comes from content: it is checked by an allow-list before any
  path or URL is built from it, and a read is bounded before its bytes are copied.
- Tests read assets without a package and without the file system, through the kit.
- The contract names no `ndk`, `objc2` or `web_sys` type (ADR-0082, ADR-0089 §2).

## Considered options

1. **The bundle is a host capability.** `flui-platform` holds the key, the reader trait and the
   handle; `flui-app` provides each host's reader from `flui-native`; `flui build` copies the
   declared files to one place in each package.
2. **Paths relative to the executable or the current directory**, the code as it stands, with
   `flui build` copying the files next to the binary.
3. **Embed every asset in the binary** (`include_bytes!` through a build script or a macro over
   `[assets]`).
4. **A bundle per subtree**, Flutter's `DefaultAssetBundle` scope above the root, which a widget
   reads with `of(ctx)`.

### Option 1, a host capability

- Good, because each platform reads its own package with its own API: the APK's compressed
  entries through `AAssetManager`, the bundle's resources through `NSBundle`, the page's files
  through the browser's fetch.
- Good, because the composition root decides the reader, a test hands the headless host its
  own, and the image cache keys a load by the bundle's identity as it keys a network load by
  `HttpAuthority`.
- Bad, because the CLI and the host must agree on a layout no type checks across them: the
  layout is §3, and a test on each side states it.

### Option 2, relative paths

- Good, because it needs no new type.
- Bad, because it cannot work on Android, iOS or the Web: none of them has a file at a path the
  application can compute from its executable, and Android's assets are entries in the APK, not
  files.

### Option 3, embed the assets

- Good, because every platform reads them the same way, from memory.
- Bad, because every asset stays in memory for the life of the process, the binary grows with
  every image, a change to one image relinks the application, and the Web's `.wasm` grows past
  what a page should download before its first frame. Flutter keeps assets out of the binary for
  the same reasons.

### Option 4, a bundle per subtree

- Good, because a subtree can read another package's assets, which Flutter's package assets
  (`packages/<name>/…`) use.
- Bad, because nothing asks for it yet: Flutter's own uses of `DefaultAssetBundle` are tests and
  package assets, the first is the headless host's (§5) and the second is a key prefix the bundle
  can serve without a scope. It can be added above the capability later without changing a key.

## Decision

Chosen option: 1, because it is the only one that reads the package each platform actually has,
and it puts the reader where HTTP, the clipboard and storage already are.

### §1 An asset is named by its key

`flui_platform::assets::AssetKey` is the asset's path relative to the project root, with `/`
between segments, as `[assets] directories` declares it: `assets/logo.png` for the file
`<project>/assets/logo.png`. It is built by `AssetKey::try_new(&str) -> Result<AssetKey,
AssetKeyError>`, an allow-list: at most 1024 bytes, at least one segment, no empty segment, no
`.` or `..` segment, and no `\`, `:`, NUL or other control character anywhere. A key that passes
is never absolute and never leaves the bundle's root, so every reader joins it to its root
without checking it again. The same key names the same asset on every platform.

### §2 The bundle is a capability of the host

```rust
pub trait AssetSource: Send + Sync + 'static {
    /// The bytes of `key`, at most `limit` of them.
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>>;
}

pub enum AssetBundleCapability {}          // Capability<Handle = AssetBundle>
pub struct AssetBundle { /* Arc<dyn AssetSource> */ }   // Clone + Send + Sync
pub struct AssetAuthority { /* Weak<dyn AssetSource> */ }  // AssetBundle::authority
#[non_exhaustive]
pub enum AssetLoadError { NotFound, TooLarge { limit: u64 }, Io(std::io::Error), … }
```

`AssetBundle::load(&key, limit)` and `AssetBundle::load_string(&key, limit)` (UTF-8, Flutter's
`loadString`) are what a widget or a service calls. A widget acquires the handle in a lifecycle
hook with `ctx.capability::<AssetBundleCapability>()`, as it acquires `HttpCapability`. The future
needs no async runtime of the caller's: a reader over a file or `AAssetManager` reads one chunk
per poll on whatever lane polls it, and the Web reader awaits the browser's fetch. A reader
checks the length against `limit` before it allocates, and again on every chunk, and opens only a
regular file. A missing asset is `NotFound`, never an I/O error with a message. The handle is
FLUI's `rootBundle`: the composition root makes one source per UI runtime and every handle it
provides clones it, never a `static`. `AssetBundle::authority` names that source by its
allocation, which it keeps reserved but not alive, so a cache keys a result by the bundle it came
from without holding the bundle.

### §3 Every package keeps asset `k` at `flui_assets/k`

`flui build` copies every regular file under each directory `[assets] directories` lists, at any
depth, to `flui_assets/<key>` inside the package's root, and the host's reader reads it there:

| Package | Root of `flui_assets/` | Reader |
|---------|------------------------|--------|
| Windows, Linux | the directory of the executable | `std::fs` |
| macOS `.app` | `Contents/Resources` | `NSBundle::mainBundle().resourcePath()` and `std::fs` |
| macOS executable outside a bundle | the directory of the executable | `std::fs` |
| iOS `.app` | the bundle's root | `NSBundle::mainBundle().resourcePath()` and `std::fs` |
| Android APK | the APK's `assets/` | `AAssetManager_open` with `AASSET_MODE_STREAMING` |
| Web | the directory of the page | the document-relative URL `flui_assets/<key>`, each segment percent-encoded, fetched through the host's `Http` |

A listed directory that does not exist, a file whose key §1 refuses, two files that map to one
key and a symbolic link are build errors that name the file. A project with no `[assets]`
section ships no assets, and its bundle answers `NotFound`.

### §4 A development run reads the project

`flui run` on a desktop target starts the application with `FLUI_ASSET_ROOT` set to the project
root, so an edited asset is read on the next load without a copy; the desktop reader reads
`$FLUI_ASSET_ROOT/<key>` when that variable is set, and its package's `flui_assets/` otherwise.
`flui run` on Android, iOS and the Web packages the assets as `flui build` does. The current
directory is never a root: an application started with plain `cargo run` and without
`FLUI_ASSET_ROOT` finds its assets only once `flui build` has copied them.

### §5 Tests hand the host their own bundle

`flui_platform::assets::DirectoryAssets`, the reader the desktop and Apple hosts build over their
root, reads a test's own directory too, and `flui_platform::testing::MemoryAssets` serves a map
of keys to bytes. The headless host provides an empty bundle unless the test names one
(`HeadlessWindow` takes it as it takes a platform convention), so a test that forgets its assets
gets `NotFound`, not a file from the checkout.

### §6 An asset image reads through the bundle

`ImageSource::Asset` holds an `AssetKey`, and its load reads through the `AssetBundle` the
widget acquired, under `MAX_ENCODED_IMAGE_BYTES`; a decoded image's cache key holds the bundle's
`AssetAuthority` beside the key. `Image::file` stays the way to read a path on the file system.
The shape of the cache and the widget constructors around it is the image cache's, not this
record's.

### Consequences

- Good, because `Image::asset("assets/logo.png")` reads the same image from an APK, an iOS bundle,
  a page and a desktop package.
- Good, because a test's assets are in the test: nothing reads the checkout by accident.
- Good, because the CLI's `[assets]` section now does what the template says it does.
- Bad, because a plain `cargo run` no longer reads assets from the current directory; `flui run`
  or `FLUI_ASSET_ROOT` is the development path.
- Bad, because the layout of §3 is a contract between `flui-cli` and `flui-native` that no type
  holds across the two; each side has a test that states it.
- Neutral, because resolution variants (`2.0x/`) and an asset manifest are not decided here: a
  manifest is added when a reader needs to list keys or pick a variant, and it lives under
  `flui_assets/` too.
- Neutral, because fonts declared in `flui.toml` are not read by this record; a font file read
  through the bundle is registered with `register_font` like any other font's bytes.

### Confirmation

- `asset_key_refuses_every_key_outside_its_bundle` (`flui-platform`) runs the allow-list of §1
  over absolute paths, `..`, backslashes, drive letters, empty segments and control characters.
- `every_asset_source_keeps_the_bundle_contract` (`flui-platform`) runs §2 against
  `DirectoryAssets` and `MemoryAssets`: a missing key is `NotFound`, a file over the limit is
  `TooLarge` before its bytes are read, a directory is `NotAFile`.
- `flui-cli`'s build tests state §3 for each package: the declared files land at
  `flui_assets/<key>`, and a refused key, a collision and a symbolic link fail the build.
- The platform readers of §3 other than Linux's are confirmed by a run on each platform, rows of
  `crates/flui-native/platform-parity.toml`.
