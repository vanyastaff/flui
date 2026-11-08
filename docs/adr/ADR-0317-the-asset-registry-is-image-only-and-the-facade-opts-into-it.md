# ADR-0317: The asset registry is image-only and the facade opts into it

- **Status:** Implemented (2026-11-07)
- **Date:** 2026-11-07
- **Supersedes:** [ADR-0120](ADR-0120-typed-asset-cache-retention.md);
  [ADR-0121](ADR-0121-owned-asset-key-names.md);
  [ADR-0105](ADR-0105-asset-validation-and-bridge-progress.md), its generic descriptor-validation
  decision; [ADR-0107](ADR-0107-asset-byte-sources-and-decoding.md), its generic `Asset` and byte-loader
  decision
- **Related:** [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md) (decoded-image ownership and
  scheduling, unchanged); [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md)
  (caller-provided lanes, unchanged); [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md) (HTTP
  capability, unchanged); [ADR-0315](ADR-0315-first-party-packages-remain-train-aligned-facade-features.md)
  (off-by-default facade aggregation)

## Context and problem statement

`flui-assets` carried two independent systems: generic, type-erased Moka caches for arbitrary
`Asset::Data`, and the decoded-image registry used by production image widgets. The first system added
keys, handles, type-state configuration, font descriptors, byte loaders, expiration and publication
rules without a production consumer outside its own tests. It also made the registry's memory contract
ambiguous: typed entries used counts and expiration while decoded pixels used a real byte budget.

The facade did not expose `flui-assets` or forward the widget asset-image feature, so an application
using only `flui` could not construct the registry required by `Image::asset` and `Image::network`.
Keeping the generic system as an additional opt-in would preserve two public loading models and their
failure matrices without supplying the trustworthy weight contract that arbitrary decoded data needs.

Which single asset API should production applications use, and how should the facade expose it without
increasing the default dependency graph?

## Decision drivers

- The frame path is synchronous; a completed-image probe and asynchronous source work are different
  operations.
- Decoded RGBA8 pixels have a trustworthy byte weight; arbitrary generic asset data does not.
- One concept has one production form. A replacement removes the old API and the documents teaching it.
- The application owns its registry. No host, runtime or process-global cache is introduced.
- Execution lanes and HTTP are caller capabilities (ADR-0279 and ADR-0280).
- Facade capabilities are absent without their features and the prelude remains feature-independent
  (ADR-0315).
- Accepted image cancellation, handover, scheduling and HTTP behavior is retained rather than redesigned.

## Considered options

1. Make `flui-assets` image-only, remove the generic cache system, and expose that registry through an
   off-by-default facade `assets` feature.
2. Keep generic typed caches and handles beside the decoded-image registry, adding only the facade edge.
3. Move the generic cache system behind another feature or into another crate while retaining the image
   registry and facade feature.

## Decision

Chosen option: 1, because the decoded-image registry is the production asset boundary, has an enforceable
memory policy, and does not need the generic system's parallel ownership and initialization rules.

### 1. `AssetRegistry` is the decoded-image registry

The crate exports `AssetRegistry`, non-generic `AssetRegistryBuilder`, `AssetError`, `Result`, and
`flui_painting::paint::Image`. With `images`, it also exports `ImageLoad`, `ImageSource`, `LoadServices`
and `NetworkSource`.

`AssetRegistryBuilder::new().build()` is valid immediately. Its optional configuration is exactly:

```rust,ignore
AssetRegistryBuilder::new()
    .with_decoded_image_budget(bytes)
    .with_decode_threads(nonzero_threads)
    .build()
```

The builder has no capacity type-state. The registry has one completed-image store: the byte-budgeted LRU
of ADR-0250. `load_image`, `load_network_image` and `cached_image` are its source operations. Moderate
`ReclaimMemory` pressure trims retained pixels to half that budget and critical pressure clears them;
caller-owned images and in-flight subscriber behavior are unchanged.

`images` remains the crate feature for decoding, file/network sources and retention. `full` remains its
alias. There is no `network` feature: `LoadServices` supplies HTTP independently of a concrete client.

### 2. Generic asset storage and loading are removed

The public `Asset`, `AssetMetadata`, `AssetCache` family, `AssetHandle` family, `AssetKey`, `FontData`,
`LoadState`, `FontAsset`, typed `ImageAsset`, `BytesFileLoader`, cache configuration/expiration types,
extension traits and `NoCapacity`/`HasCapacity` markers are removed, together with generic registry
`load`, `get`, `preload`, `invalidate`, `clear` and `clear_all` methods.

The implementation removes type-erased `TypeId` cache storage, Moka and Tokio filesystem loading. The
private image decoder moves under `registry`; its checked dimension arithmetic, allocation limit and
`image` decoder-hook behavior remain. File input continues through bounded `std::fs` chunks on the IO
lane or polling thread. `lru`, `image`, Tokio synchronization and caller-provided lane/HTTP contracts
remain because the live registry uses them.

This decision does not introduce a generic cache feature, deprecated duplicate or a new home for the
removed abstraction. A future arbitrary-asset proposal needs its own production consumer, ownership
model and trustworthy memory-weight decision.

### 3. The facade exposes exactly one opt-in image feature

The root package declares an optional `flui-assets` edge and:

```toml
assets = ["dep:flui-assets", "flui-assets/images", "flui-widgets/asset-images"]
```

With that feature, `src/lib.rs` exposes `pub use flui_assets as assets`. Without it, `flui::assets` is
absent and neither `flui-assets` nor Moka enters the ordinary graph. The feature also makes the existing
`Image::asset` and `Image::network` providers available. The facade prelude and SDK surface do not
change, and `flui-app` acquires no registry ownership.

`cargo xtask facade-combos` compiles `--no-default-features --features assets` in isolation so this edge
cannot be masked by workspace feature unification.

### 4. Failure and cancellation behavior stays at the image boundary

Failures remain typed `AssetError` values and are not cached. Missing HTTP reports `HttpUnavailable`;
transport and non-2xx results report `Http` and `HttpStatus`; source/decode failures report `LoadFailed`
or `DecodeFailed`; retiring lanes report `LanesRefused`. A later request can retry.

A decoder panic is contained as a failed image load. Dropping one subscriber leaves shared work alive;
dropping the last cancels it. An unfinished run can be taken over by a remaining waiter on that waiter's
services, and ticket identity prevents stale completion into a newer generation. Wakers, lane submission,
completion, cancellation and dropped pixels run after internal state locks are released.

### Consequences

- Good, because applications have one image cache, one source identity and one byte-memory policy.
- Good, because a facade-only consumer can construct and mount real asset/network images with one
  feature, while the default graph remains free of asset machinery.
- Good, because the removed generic reentry, singleflight, expiration and handle-lifetime matrices no
  longer need maintenance without a production consumer.
- Bad, because users of the removed generic/font APIs must own a different domain-specific solution;
  there is no compatibility wrapper.
- Bad, because facade semver now includes `flui-assets` when `assets` is selected.
- Neutral, because the crate's existing wasm support metadata and platform-support claims do not change.

### Confirmation

- `assets_feature_exposes_and_loads_images_through_the_facade` builds an external consumer, mounts a real
  `Image::asset` through the facade, drives it to its actual decoded dimensions, checks the completed
  registry entry and proves `flui-assets` is present while Moka is absent.
- `assets_module_is_absent_without_its_feature` proves `use flui::assets::AssetRegistry` is an E0432
  resolution failure without `assets`.
- `ordinary_facade_graph_excludes_test_support` also excludes `flui-assets` and Moka from ordinary
  consumer graphs.
- `decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure`,
  `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave`, the network-load suites and the
  real decoder-hook scheduling suite retain ADR-0250/0279/0280 behavior.
- `decoded_pixels_are_bounded_before_decoding` retains header/allocation bounds after the decoder moves.
- `cargo xtask facade-combos`, `workspace`, `reach` and clippy enforce the feature edge, dependency graph
  and warning-free public surface.

## Pros and cons of the options

### 1. Image-only registry with facade opt-in (chosen)

- Good, because it matches the production consumer and removes a second cache and ownership model.
- Good, because byte-budgeted decoded pixels have a direct, testable memory contract.
- Good, because the default facade pays nothing and the opt-in surface is small.
- Bad, because removal breaks generic asset users instead of offering a migration alias.

### 2. Keep both registry models

- Good, because existing generic users keep their API.
- Bad, because two completed stores and loading/failure contracts remain, including a generic cache with
  no production consumer or reliable byte weight.
- Bad, because facade exposure would make the unused abstraction a supported application API.

### 3. Move generic caching behind another feature or crate

- Good, because the default image path could shed Moka while preserving a migration destination.
- Bad, because it retains the old form, tests and documentation without resolving its missing consumer
  and memory-policy problems.
- Bad, because another feature or crate would encourage new users to copy an abstraction this decision
  finds unjustified.

## More information

ADR-0250, ADR-0279 and ADR-0280 remain authoritative for decoded-image retention/scheduling, execution
lanes and HTTP respectively. Reintroducing arbitrary asset types requires a new ADR with a production
caller and an explicit memory-weight and failure contract, not restoration of the removed generic code by
default.
