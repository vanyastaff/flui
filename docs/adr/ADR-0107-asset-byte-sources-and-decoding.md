# ADR-0107: Image byte sources and decoding

- **Status:** Accepted (2026-10-03)
- **Date:** 2026-10-03
- **Superseded-by:** [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md) §5 for HTTP transport;
  [ADR-0317](ADR-0317-the-asset-registry-is-image-only-and-the-facade-opts-into-it.md) for generic
  `Asset` implementations and byte loaders
- **Related:** [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md),
  [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md)
- **Revised:** 2026-11-07 — states the concrete file/network sources and private bounded decoder

## Context and problem statement

A key-only generic loader cannot choose an arbitrary asset's byte source or decoder. The old file and
network implementations were unsupported stubs, while embedded constructors duplicated another source
abstraction. Production image widgets now use concrete file and network sources through the registry.
The current decision identifies those sources, their feature boundary and the decoder they share.

## Decision

`AssetRegistry::load_image` reads a regular file in bounded chunks and decodes it. There is no public
`Asset::load`, `AssetLoader`, `BytesFileLoader`, `ImageAsset` or embedded-image descriptor. An
application with bytes already in memory uses the widget's memory-image provider or its own domain code;
the registry does not invent a cache key for opaque bytes.

`AssetRegistry::load_network_image` obtains encoded bytes through the `Http` client in `LoadServices`,
as ADR-0280 decides. The crate contains no concrete HTTP stack, `reqwest`, `NetworkLoader` or `network`
feature. Missing HTTP is `HttpUnavailable`; failed requests and non-2xx responses are typed and not
cached. URL plus client authority forms network source identity.

Both sources call one private `registry::decode` implementation using the `image` crate. It preserves
registered decoder hooks, checks header dimensions with checked arithmetic, applies the decoder
allocation limit, and produces `flui_painting::paint::Image` RGBA8 pixels. Encoded files and bodies are
capped at 256 MiB. Decoded pixels and decoder allocation are capped at 256 MiB, or 128 MiB on Android,
iOS and the web.

The `images` feature gates the registry's file/network loading and decoding API. Disabled functionality
is absent rather than a callable stub. `flui-widgets/asset-images` enables `flui-assets/images`; the
facade's `assets` feature forwards both edges.

## Behavioral evidence

- `image_file_loads_a_committed_png_to_its_real_dimensions_and_recovers` decodes a committed PNG through
  `load_image`, checks its real dimensions and RGBA8 length, and recovers after a missing file.
- `decoded_pixels_are_bounded_before_decoding` refuses a header whose declared RGBA8 size exceeds the
  cap before allocating those pixels.
- `network_loads_fetch_with_the_callers_client_and_recover_after_errors` checks the caller's client,
  body bound, typed failures and retry.
- `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` checks production file loading
  with lanes and without Tokio.

## Consequences

Source selection is explicit at the registry method, and decoding has one implementation for files and
HTTP. There is no extension point for arbitrary typed assets or source-specific generic caches.
Applications select `images` directly or facade `assets`; network availability still depends on the HTTP
capability supplied by the host or test.
