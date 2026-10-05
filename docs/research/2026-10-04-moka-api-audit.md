# Moka API audit, 2026-10-04

## Sources and version decision

Reviewed [Moka 0.12.16](https://docs.rs/moka/0.12.16/moka/),
[future Cache](https://docs.rs/moka/0.12.16/moka/future/struct.Cache.html),
[CacheBuilder](https://docs.rs/moka/0.12.16/moka/future/struct.CacheBuilder.html),
[borrowed entry selectors](https://docs.rs/moka/0.12.16/moka/future/struct.RefKeyEntrySelector.html)
and owned selectors/Entry, alongside the published crate's source, feature manifest
and exact [upstream changelog](https://github.com/moka-rs/moka/blob/a616ec19e8d4ed938caf8b2c88090331d778d5da/CHANGELOG.md).

The lockfile already selects 0.12.16; the workspace minimum now requires it.
Its insertion/removal race fix matters even under the default TinyLFU policy:
earlier versions could retain phantom slots, over-report counts and progressively
lose usable capacity. Under Moka's optional LRU policy, that race could stall
eviction entirely. The same release raises crossbeam-epoch's advisory-safe minimum
and repairs a ThreadSanitizer false positive; Moka itself does not exercise the
advisory's pointer-formatting path. Version 0.12.15 also repaired expired-entry
reinsertion under custom Expiry, which FLUI does not enable.

## API-family decisions

| Family reviewed | FLUI decision |
| --- | --- |
| Cache clone, name, policy, new/builder | Clones share data and counters; read real policy capacity. No unused naming surface. |
| contains_key, get, borrowed keys and Equivalent | Presence must be observational; retrieving reads update policy. Asset handles own their keys, so retain the typed key contract. |
| get_with/by_ref/if, optionally_get_with/by_ref, try_get_with/by_ref | Built-in loading is fallible; use borrowed fallible entry initialization to additionally inspect freshness. No negative caching or automatic refresh contract. |
| entry/entry_by_ref, or_default, or_insert, or_insert_with/if, optional/fallible insert | Borrowed or_try_insert_with coalesces closed font cold work; generic callbacks use independent initialization. Owned selectors clone keys earlier without benefit here. |
| and_compute_with, and_try_compute_with, and_try_compute_if_nobody_else, and_upsert_with | Serialize per-key transformations; FLUI has immutable asset snapshots, not a mutable/refreshing asset contract. Avoid unwired update APIs. |
| Entry key/value/into_value, is_fresh and replacement metadata | Fresh returned results count completed insertions; warm hits and waiting cold callers are distinguished by the initial presence probe. |
| insert, invalidate, remove, invalidate_all | Keep explicit insertion/invalidation. Remove clones a discarded value and is unnecessary. Invalidation neither cancels pending loads nor releases all consumer ownership. |
| invalidate_entries_if, PredicateId/Error | Requires extra builder state and callback lifecycle. No production predicate invalidation need; leave disabled. |
| iter/IntoIterator | Weak concurrent iteration does not promote keys; unnecessary for existing cache operations. No snapshot guarantee invented. |
| entry_count, weighted_size, run_pending_tasks, debug_stats | Entry estimates and maintenance are useful. Debug stats are feature-gated unstable instrumentation; no dependency on them. No byte claim from unweighted size. |
| max/initial capacity, weigher, eviction_policy | Explicit count bound; preserve TinyLFU default. No guessed 10 KiB weight, arbitrary preallocation or switch to Moka LRU. A trustworthy per-data weight contract must precede weighting. |
| time_to_live, time_to_idle, expire_after/Expiry | Expose bounded lifetime/idle intervals and explicit absence. No caller requirement for custom per-entry callbacks. Check Moka's 1,000-year limit before build. |
| eviction_listener, async_eviction_listener, RemovalCause | Listeners receive owned clones, may run during maintenance, and are disabled after a panic. Rename request counters accurately rather than adding an unnecessary callback. |
| build/build_with_hasher, hashing | Keep RandomState for externally influenced asset keys. Faster hashing is not justified by measurements. |
| sync/SegmentedCache, future, logging, quanta, atomic64, unstable-debug-counters | Only future is selected. Sync caches cannot provide asynchronous cold-load coalescing. Other features are unnecessary; atomic64 is a compatibility no-op. |

Read/write recording and maintenance are eventually consistent. Writes can await
maintenance capacity; the wrapper also uses locks for statistics and registry maps.
This is not a completely lock-free API. Arc values avoid cloning decoded payloads.
Moka initializes per key and error type; failed initializers are not stored.
Generic user work cannot safely join pending same-key work: native spawned children
expose no parent ancestry to the cache. The public helper therefore retains its
pre-audit get/initialize/insert behavior and owned error type. Every successful
call publishes; later completion may overwrite data without revoking handles.
Custom registry loading follows this path, intentionally changing its previous
singleflight promise. The closed FontAsset producer retains private Moka
singleflight behind exact TypeId checks, with no unsafe casts or loader duplication.
The active-key bypass candidate was rejected because it also duplicated expensive
built-in loads; its green verification does not establish the final implementation.

The reentry repair was checked with two serial production controls. Disabling
the same-key bypass makes the real public reentry row time out after five
seconds. Disabling retirement ancestry lets cancellation's destructor publish
its nested result, and the real cancellation row fails because the cache is no
longer empty. Each row was run through a temporary filterable test wrapper;
neither its inputs nor assertions were changed. Exact production and test bytes
were restored in `finally` before subsequent compilation. The restored
full-feature suite passes all 14 tests, and all ten feature-powerset/all-target
Clippy configurations pass with warnings denied.
Cancellation or panic allows another caller's initializer to retry, with a bounded
upstream retry policy. Callers must tolerate restart and must not treat clear as
cancellation. Generic destructor retirement is outside the registry lock, without
a new guarantee of containing panicking destructors.

## Consumer coverage ledger

| Consumer | Reading and changes | Compilation / execution |
| --- | --- | --- |
| flui-assets cache/config/stats | Count capacity, checked expiration, shared public initialization, presence, metrics and retirement contracts | All-target Clippy in 10 feature configurations; full-feature 14 tests and no-default-feature 8 tests passed on Windows |
| flui-assets registry | Unified initialization preserves validation and owned AssetError; builder configuration preserves HTTP/runtime owners | All-target Clippy in 10 feature configurations; full-feature 14 tests and no-default-feature 8 tests passed on Windows |
| flui-assets public tests | Count/disabled/expiration, shared allocation/non-Clone error, cancellation/panic/retry, observational counters and bounded destructor reentry | Prior audit suite 14 passed; final repair verification below |
| flui-assets images/network/default/full | All declared optional paths, test modules and library examples reviewed | 10 feature configurations compiled; default/full tests executed; full-feature doctests 16 passed and 50 ignored |
| flui-assets documentation | README, GUIDE, PERFORMANCE, PATTERNS and architecture; corrected obsolete byte hints and unsupported claims | Strict source gates passed |
| widget image cache / bridge | Synchronous LRU probe and subscription policy; bridge calls direct image load and bypasses typed caching | Documentation corrected; dependent whole-workspace Clippy and 652 tests passed |
| root manifest/tooling | Future-only Moka minimum, locked version already current; no shader or benchmark directly consumes Moka | deps --strict and whole-workspace check-changed passed |

Moka stores typed data, including decoded images. The widget bridge bypasses that
cache and hands completed decodes to a synchronous LRU. Explicit independent
registry and widget loads can hold separate allocations. There is no combined
memory bound or universal sharing guarantee. Keep the distinct ownership and
frame-path boundaries.

## Regression evidence and limits

Public tests exercise production producers, not copies of policy predicates.
Restoring map destruction under the registry guard makes destructor reentry
time out. Restoring check/load/insert makes concurrent loaders run twice; restoring
the minimum of 100 entries violates the two-entry test. Presence counter pollution
is separately restored as a narrow control of the old observational defect.
Each control runs serially, preserving exact production bytes in a finally path.

Wrong historical utilization and overflowing integer-rate arithmetic also fail their real consumer cases. Whole-workspace check-changed passed: 652 tests passed/10 skipped, strict private rustdoc, 619 doctests passed/388 ignored, available Windows/macOS/WASM compilation. flui-assets itself was compiled and executed on Windows; the broader cross-target gates do not establish another platform execution for Moka. assets_basic_usage ran successfully with full features (font allocation sharing, decoded image, invalidation and preload). Android/iOS targets and the Linux xvfb platform suite were unavailable locally. No benchmark,
interactive example, absent-target runtime or general memory-performance claim
follows from reading or compilation.

Repair verification: cargo xtask check-changed --base
7a477e972032ecfc076e3f98a89701d0bdbc76cb passed over flui-assets and
20 dependent packages. Nextest executed 292 tests: 292 passed, zero skipped.
Strict private rustdoc passed; doctests passed 236 cases with 178 ignored.
The gate compiled the available Windows CLI and WASM paths and passed both
30-configuration each-feature Clippy matrices (library/binaries and
tests/benches/examples). Android and iOS runners were unavailable; cross-target
compilation is not evidence of asset-cache execution on another platform.
The repair's independent asset feature powerset passed all 10 all-target
configurations; default/full tests and both defect controls executed on Windows.

## Spawned-child progress repair

A new two-worker public row on unchanged production failed with the intended
five-second timeout: an initializer spawned a same-key child and awaited its
JoinHandle. Poll-scoped ancestry was insufficient. The final design separates
arbitrary user initialization from closed built-in singleflight. Public rows pin
native spawn, recursive custom Registry loading, late publication overwrite,
owned non-Clone errors, cancellation, panic, retirement and healthy retry.
The real Font producer row gates runtime-local IO to prove cold overlap; a
private Moka-helper matrix controls failure and elected cancellation/panic.
Earlier counts above describe prior repairs; final verification follows.

### Verification before the decoder-hook boundary correction

Restored full-feature tests: 15 passed (4 unit/private, 11 integration); full
doctests: 16 passed, 50 ignored. Default tests: 9 passed; default doctests:
15 passed, 45 ignored. All ten feature-powerset/all-target Clippy configurations
passed with warnings denied. These results are from the final generic-independent,
built-in-singleflight implementation, after both controls restored exact bytes.

Two serial controls use the unchanged public rows via temporary filter wrappers.
Bypassing the exact built-in whitelist makes both real cold Font and Image rows
fail their shared-allocation assertion. Routing public initialization through the
private Moka helper restores the same-key spawned-child self-wait and fails with
the intended five-second timeout. For this narrowly restored success-only defect,
Arc::try_unwrap converts the private error type back to the public owned signature;
that conversion is unreachable in the row. Production and test bytes are saved
before each mutation and restored in finally before the next build. Temporary
wrappers are removed, and the restored full suite passes. The private failure
matrix separately verifies shared errors and elected cancellation/panic recovery.

Final source checks and dependent gate are pending. Windows executes the public
and private cache behavior; GPU and other native execution are outside this repair.

### Actual decoder callback boundary

Locked image0.25.10 load_from_memory uses ImageReader.with_guessed_format, which
consults registered format detection hooks and can invoke a registered decoder.
Those hooks are arbitrary user code. ImageAsset therefore stays on independent
initialization, preserving its registered decoder semantics. No image API or
format support is narrowed to make singleflight possible. FontAsset instead
reads bytes via BytesFileLoader -> tokio::fs::read -> standard filesystem IO;
FontData::from_bytes only wraps the Vec in Arc. It invokes no custom asset or
font decoder callback. The exact FontAsset TypeId alone enables private Moka
singleflight. The process-isolated real image hook regression checks an awaited
same-key spawned child and caps its synchronous bridge at three seconds.

The previous 15-test verification and interrupted dependent gate belong to the
Font/Image whitelist revision and do not establish this final font-only boundary.
Final font-only full suite: 16 tests and 16 doctests passed (50 ignored). Three final serial controls failed intentionally: Font whitelist bypass violated
public cold allocation sharing; routing generic initialization through Moka
timed out after five seconds; adding ImageAsset to the whitelist made the real
registered decoder hook child time out after three seconds. Exact source/test
bytes were restored in finally, including after a rejected control attempt with
Windows newline translation. Default/features/source/dependent gates are pending.

### Final font-only gate

After all three controls restored exact bytes, the full suite passed 16 tests
and 16 doctests (50 ignored); defaults passed 9 tests and 15 doctests (45 ignored).
All ten feature-powerset/all-target Clippy configurations passed with warnings
denied. Source checks passed with strict mode. check-changed against the preceding
repair passed all 293 dependent tests with no skips, 236 dependent doctests
(178 ignored), strict private rustdoc and both 30-configuration feature passes.
These counts apply to the final font-only producer boundary, not earlier candidates.
Windows behavior was executed; GPU and other native execution are outside this
cache repair. Future callback-free image singleflight would need its own explicit
producer contract and is not introduced here.
