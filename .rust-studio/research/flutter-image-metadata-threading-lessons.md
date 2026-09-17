QUESTION: Does Flutter #177328 imply a current FLUI issue around image metadata/decode work on the UI or platform thread?

ANSWER: No confirmed current FLUI defect. FLUI's framework-managed `AssetImage`/`NetworkImage` decode path runs through `flui-assets`' bridge runtime and `flui-widgets`' coalesced decode cache; selected image async tests pass. The lesson is a future guardrail: if FLUI adds cheap image-dimensions metadata, it must not parse heavy platform metadata synchronously on the UI/platform lane.

EXTERNAL SOURCE:
- Flutter issue #177328, open as of 2026-09-13: image metadata queries through iOS platform codecs can be slow on the platform thread. The useful architectural lesson from the comments is to separate cheap header/dimension extraction from full metadata/decode work, and to move expensive platform-codec metadata work off the platform thread.

LOCAL SOURCES:
- `crates/flui-assets/src/assets/image.rs:85-107`: image asset loading reads bytes asynchronously and decodes with `image::load_from_memory`, then converts to RGBA.
- `crates/flui-assets/src/assets/image.rs:117-129`: current `ImageAsset::metadata` extracts only extension/size information; it does not synchronously decode dimensions.
- `crates/flui-assets/src/registry/mod.rs:119-176`: `load_image_bridged` spawns file read + decode onto a resolved tokio runtime, never on the caller's thread.
- `crates/flui-assets/src/registry/mod.rs:182-224`: `load_network_image_bridged` fetches and decodes on the same background runtime.
- `crates/flui-assets/src/registry/bridge.rs:1-85`: bridge runtime is explicitly a background tokio runtime, lazily started or injected.
- `crates/flui-widgets/src/image/decode_cache.rs:1-48`: decoded-image cache coalesces concurrent loads and removes abandoned pending loads.
- `crates/flui-widgets/src/image/resolve.rs:1-39`: image resolver cancels on swap/dispose and generation-guards completions.
- `crates/flui-widgets/src/image/provider.rs:267-355`: `MemoryImage`/`FileImage` remain synchronous and document blocking decode per call; this is a user-chosen provider path, not the async asset/network path.

TESTS:
- `cargo nextest run -p flui-widgets --features asset-images,images --test image_async --test image_network --test image --no-fail-fast`
- Result: 32 passed, 0 skipped.

DISPOSITION:
- No new issue filed. Existing image/resource issues (#1061/#1062/#1063) cover byte-cache budget, cancellation, and CPU/I/O separation classes. This pass did not find a distinct current metadata-on-UI-lane defect.

OPEN:
- If FLUI later adds an image metadata API that returns dimensions without full decode, require tests proving it does not perform heavyweight platform metadata extraction on the UI/platform lane.

ANSWERED
