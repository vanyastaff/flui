# lru API audit

The workspace's only direct `lru` consumer is the optional decoded-image cache
in `flui-widgets`, enabled by `asset-images` and therefore also `network-images`.
The inventory includes production code, tests, examples and benchmarks. The
previous reqwest audit is separate; completed HTTP cache reuse depends on its
registry identity repair.

## Documentation and version

Read the docs.rs overview and complete `LruCache` method inventory, iterator
interfaces, locked source and changelog. The initial lock selects 0.18.4;
crates.io confirms 0.18.5 is current stable. The workspace requires at least
0.18.5 and Cargo regenerates the lock accordingly. Its upstream change narrows hashbrown's optional features to
`default-hasher` and `inline-more`; the workspace stays on stable Rust and does
not request `nightly`.

## API decisions

| API family | Application to FLUI |
|---|---|
| `new`, `sparse`, `with_hasher` | Keep eager capacity reservation for the small cache. Encode its nonzero capacity in `NonZeroUsize` instead of silently converting zero to one. `sparse` is useful for large potential capacities with sparse use, which this cache does not have. No measured workload justifies replacing the maintained default hasher. |
| `unbounded`, `unbounded_with_hasher` | Do not remove the declared count bound. Unbounded construction is not equivalent to a zero-capacity or disabled cache. |
| `get`, `get_mut`, `get_key_value`, `get_key_value_mut` | `get` refreshes recency and a cheap image handle clone releases the borrow before returning. A completed async admission also performs that lookup: an earlier miss at the widget does not establish that the key is still cold when it subscribes. Mutating image buffers or returning references under the cache lock is unnecessary. |
| `peek`, `peek_mut`, `peek_lru`, `peek_mru`, `contains` | These leave recency unchanged. They serve inspection; replacing a consumer hit with `peek` would evict an image that was just used. No telemetry or public inspection API currently requires them. |
| `put`, `push` | `put` returns an old value on replacement but discards capacity-evicted entries internally. `push` returns both key and value for replacement or capacity eviction. Use `push`, leave the guard scope, then retire that ownership so freeing the last large pixel allocation does not hold the process-wide cache lock. |
| `get_or_insert*`, `try_get_or_insert*` | Owned/borrowed-key, with-key and mutable-result variants handle synchronous initialization; fallible variants do not insert on failure. Borrowed variants allocate the owned key only on a miss. They cannot replace async coalescing: awaiting a file/HTTP load or invoking user initialization under the entries lock would put I/O and callback lifetime into the locking protocol. |
| `pop`, `pop_entry`, `pop_lru`, `pop_mru` | `pop_entry` and end-pop methods return key and value ownership. A future explicit eviction policy must retire the returned pair after unlocking. No public invalidation or memory-pressure service exists to wire a new eviction API now. |
| `promote`, `demote`, `find_and_promote` | Explicit recency changes and predicate search do not improve the existing key lookup. `find_and_promote` searches MRU to LRU, unlike constant-time keyed access, and a predicate would execute while the cache is borrowed. |
| `retain`, `resize`, `clear` | `retain` visits MRU to LRU, preserves kept order and may mutate values; rejected entries are destroyed in the operation. Shrinking `resize` and `clear` also retire entries internally. Do not call them under a lock guarding user-owned destructors. The only current `clear` is test isolation with concrete image/string keys. |
| `len`, `is_empty`, `cap` | Capacity counts entries, not image bytes or handles retained by displayed widgets. Do not describe it as a process memory budget. |
| Iteration and cloning | Borrowed iteration is MRU to LRU, double-ended and exact-sized; mutable iteration does not itself promote entries. Consuming iteration transfers ownership in LRU-to-MRU order in the audited 0.18.5 source (next calls pop_lru), and dropping the iterator retires its remaining entries. Cloning a cache clones keys/values into another independent LRU; cloning an image handle shares its pixel allocation. The UI needs the latter. |
| Threading and panic behavior | `LruCache` provides conditional Send/Sync bounds, not shared mutation synchronization. The existing infrastructure mutex remains necessary. Source review distinguishes key/value destruction and callback execution; these concrete pixel/key types have no user-defined destructor, so the eviction change is an ownership/lock-duration improvement, not a proven callback deadlock repair. |

The crate provides fast count-bounded recency, borrowed key lookup and explicit
ownership-returning eviction. It does not provide async load coalescing, a byte
budget, TTL, or automatic invalidation when a source changes. FLUI deliberately
keeps successful decoded images until count-based eviction; an already displayed
image handle remains valid after eviction. Failed loads do not become completed
entries and a later request may retry.

## Consumer coverage and behavioral verification

| Surface | Read | Compilation | Execution |
|---|---|---|---|
| Decoded cache and image resolver | Complete cache/admission/subscriber paths and architecture | Eight per-feature library Clippy configurations passed | Local LRU/coalescing contracts passed |
| Asset and HTTP image providers | Complete constructors, sync probes and async resolutions | asset-images and network-images included in feature checks; strict private rustdoc passed | Real temporary-file loads and hermetic HTTP server tests passed |
| Tests | Existing coalescing/cancellation/async widget tests plus new cache reuse, eviction and reentry cases | Eight per-feature test/example/bench Clippy configurations passed | Affected network-images suite: 14 passed; optional-feature doctests: 29 passed, 15 ignored |
| Examples/benchmarks | Search finds no direct LRU call beyond the cache; optional feature manifests reviewed | Included in the eight per-feature target checks | Interactive examples and benchmarks have not run |

`asset_image_async_reuses_completed_decodes_after_cold_failure_recovery` removes
a real PNG source after successful decoding, then repeats async resolution. The
bridge directly loads the descriptor, bypassing the asset registry's Moka cache,
so a success establishes decoded-cache reuse rather than an upstream byte hit.
The cold-error row verifies retry after the source becomes available.

`decoded_cache_promotes_hits_and_preserves_displayed_pixels_after_eviction`
uses the private local-cache seam with capacity two to check recency, bounded
eviction, replacement and live displayed handles without making the global
capacity a consumer contract. The private coalescing family also retires unused
load captures through the public asset provider on both cached and pending hits;
neither cache guard may remain held during that reentry.

Both production defect controls failed for the intended reason: disabling the
completed-entry admission lookup made both public rows attempt to read the
removed source; replacing `get` with `peek` failed the least-recent eviction
assertion. Each mutation ran serially, with exact source bytes restored in a
`finally` block before another build.

`cargo xtask check-changed --base origin/main` passed over the whole workspace:
workspace and engine/testing Clippy, 650 nextest tests passed with 10 skips,
strict private rustdoc, and 619 doctests passed with 389 ignored. Windows and
macOS platform/tooling Clippy and the WASM workspace/facade checks passed.
`cargo xtask deps --strict` passed bans, licenses, sources, advisories and
unused-dependency checks.

Execution was on Windows, including the temporary-file, local HTTP and engine
readback cases. Cross-target Clippy establishes compilation only. Android/iOS
targets were absent; the iOS runner also requires macOS. The Linux/headless
platform suite needs Linux and xvfb-run and was not executed here. Interactive
examples and benchmarks were compiled but not run. No snapshots changed and no
library other than lru has been started during this audit.

## Sources

- <https://docs.rs/lru/0.18.5/lru/>
- <https://docs.rs/lru/0.18.5/lru/struct.LruCache.html>
- <https://docs.rs/lru/0.18.5/lru/struct.Iter.html>
- <https://docs.rs/lru/0.18.5/lru/struct.IterMut.html>
- <https://docs.rs/lru/0.18.5/lru/struct.IntoIter.html>
- <https://docs.rs/crate/lru/0.18.5/source/CHANGELOG.md>
- <https://docs.rs/crate/lru/0.18.5/source/Cargo.toml.orig>
