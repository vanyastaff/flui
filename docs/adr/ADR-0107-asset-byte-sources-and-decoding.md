# ADR-0107: Asset byte sources and decoding

- **Status:** Accepted
- **Date:** 2026-10-03
- **Related:** [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md)

## Context

The registry and image providers load concrete `Asset` values. A parallel public
`AssetLoader<T>` trait accepted only a key, which does not supply an arbitrary
asset's decoder. Its generic file implementation read bytes and then always
returned an unsupported-operation error; its network implementation always
returned the same kind of error. `MemoryLoader` had no production consumer and
duplicated the embedded-byte constructors and registry storage boundary.

## Decision

Remove `AssetLoader`, the unsupported generic `FileLoader` and network loader
implementation, and `MemoryLoader`. Custom assets implement `Asset::load`, choosing
their byte source and decoder explicitly. `BytesFileLoader` performs real file
reads and is used by the file branches of `FontAsset` and `ImageAsset`. The network
bridge retains `NetworkLoader::load_url`, then decodes the fetched bytes through
`ImageAsset`. Embedded assets own their bytes through existing `from_bytes`
constructors. The registry caches decoded data; no additional storage or decoder
trait is introduced. Image decoding continues to use the mature `image` crate,
and HTTP transport uses `reqwest`.

Image modules/types require `images`; network modules/types require `network`.
Disabled functionality is absent from the compiled API. There are no callable
feature-disabled stubs that perform I/O and then return an unconditional error.
Widget `asset-images` enables `flui-assets/images`; `network-images` additionally
enables `flui-assets/network`, preserving these production paths. The consumer
audit used `rg -n 'ImageAsset|NetworkLoader' --glob '*.rs' --glob 'Cargo.toml'`:
the registry bridge paths and executable test/example imports already carry the
corresponding feature gates.

## Behavioral evidence

- `image_asset_file_loads_a_committed_png_fixture_to_its_real_dimensions` compares
  real file and embedded decoding, rejects malformed embedded data without caching
  it, accepts valid bytes under the same key afterward, and recovers after a missing
  file. `load_image_bridged_completes_both_the_success_and_the_failure_path` covers
  the production bridged file path and non-Tokio polling.
- `font_sources_preserve_bytes_and_recover_after_load_errors` compares file and
  embedded font bytes, rejects short/invalid data without caching, and exercises
  subsequent success after invalid data and missing files.
- `load_url_round_trips_bytes_from_a_hermetic_local_server` retains real HTTP
  transport coverage for the network loader.

## Consequences

Consumers implementing the removed generic loader trait migrate source selection
and decoding into `Asset::load`. Memory sources use concrete embedded constructors
or their own `Asset` implementation. Consumers naming optional types must enable
their features. Existing decoded-data and bridge ownership contracts remain those
of `Asset` and ADR-0105.
