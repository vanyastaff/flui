# Dependency API audit

Baseline: `f7d49b596`, Rust 1.99.0, Windows x86_64 MSVC. The inventory from
`cargo metadata --no-deps --locked --format-version 1` contains 39 workspace
members and 119 distinct direct external libraries, including target-specific,
build and development dependencies. A library is completed before starting the
next one; reading a supporting API does not complete its separate audit.

## reqwest

### Documentation and consumer coverage

The locked version is 0.13.5; the docs.rs `latest` page's HTML metadata confirms
the same version, so a version bump adds nothing. Read the crate overview, `Client`, the complete
`ClientBuilder` method documentation, `RequestBuilder`, `Response`, `Error`,
redirect policy, retry defaults, TLS backends, proxy configuration, `Body`, and
the blocking, DNS, multipart and cookie module overviews on docs.rs.
Cross-check feature metadata against the locked crate's `Cargo.toml.orig`:
the docs.rs feature page omits `cookies`, although the source defines it.

All Rust and Cargo manifest references were located using
`rg -n 'reqwest|NetworkLoader|load_network_image_bridged'` over the repository.
Direct consumers are `flui-assets` and the facade's development dependency for
`examples/ai_streaming.rs`. The widget `NetworkImage` reaches HTTP through its
explicitly owned asset registry. Its synchronous resolve only probes decoded
data; HTTP remains on the background runtime. The network-loader example and
existing hermetic asset/widget tests were read separately.

### API decisions

| API family | Application to FLUI |
|---|---|
| Client ownership and pooling | A client is cheaply cloneable and already shares its internal pool. One client per image request defeats connection reuse. Registry ownership should preserve the pool across loads, including after decode failure, without adding another lock around each request. |
| Fallible construction | `Client::new()` can panic on TLS/resolver initialization. The production registry path must use `ClientBuilder::build()` and return its failure through the existing asset error boundary. Failed lazy initialization must remain retryable. |
| Request bodies and authentication | The provider example uses JSON serialization and bearer authentication. Its explicit HTTP(S) scheme validation, credential-free URL and HTTPS requirement for keys are deliberate. Form, query and multipart APIs serve no current consumer. |
| Response status | Preserve the loader's success-only 2xx test. `error_for_status()` rejects only 4xx/5xx and would accidentally admit unfollowed redirects. Do not silently replace one with the other. |
| Response bytes and text | Assets need exact binary bytes. Preserve strict `String::from_utf8` for `load_text`; `Response::text()` always replaces malformed UTF-8; with `charset` enabled it additionally processes Content-Type encodings and strips the BOM. Without `charset` it preserves the BOM, but remains lossy. The loader contract must stay independent of feature unification. |
| Incremental response bodies | SSE needs incremental byte framing, not `text()` or `json()` on the full response. `Response::chunk()` is available without the `stream` feature; `bytes_stream()` is appropriate when actual Stream composition is needed. Chunk boundaries are not UTF-8, line or event boundaries. |
| Deadlines | Total timeout covers connect through body completion; read timeout resets after each successful read; connect timeout covers only connection establishment. Existing provider total/connect deadlines have different purposes. A caller-supplied loader client already allows custom policies. General asset size/deadline policy needs an explicit contract rather than an unexplained limit. |
| Redirects | Image GETs retain reqwest's bounded default policy. The credential-bearing provider deliberately disables redirects. Custom policies must enforce their own bound. |
| Retry | Retain the default safe protocol retry classification; do not retry arbitrary provider POSTs or 5xx without server idempotency knowledge. A custom classifier overrides the default and must explicitly restore wanted behavior. |
| TLS | Explicit Rustls and disabled default features keep the requested backend and optional functionality deliberate. In locked 0.13.5, `default-tls = ["rustls"]`; the workspace comment claiming native-tls is incorrect. TLS certificate/hostname verification remains enabled. No preconfigured backend or insecure certificate override is warranted. |
| Proxies and DNS | Native Windows/macOS system settings require `system-proxy`; environment proxy discovery is still part of the native connector when this feature is off. Keep externally supplied client configuration. Do not replace the resolver or bind interfaces without a host requirement. |
| HTTP versions and sockets | Current graph requests HTTP/1, with no HTTP/2 feature. HTTP/2 tuning, connection windows, keepalive and connector layers need a measured workload before changing library defaults. Experimental HTTP/3 requires unstable cfg and is unsuitable as an incidental migration. |
| Compression and cookies | Opt-in decompression changes body size and may remove Content-Length. Cookie persistence is unnecessary for anonymous image loading and provider bearer auth. No activation without an actual requirement. |
| Blocking and wasm | Blocking HTTP is unsuitable for the frame path. `flui-assets` declares wasm unsupported because it owns native Tokio services; browser reqwest uses Fetch with different TLS/cookie/client controls. Native coverage does not establish browser coverage. |
| Other surface | Request inspection/build_split/try_clone, Body streaming, upgrades, response extensions, multipart forms and custom DNS/Tower connectors were considered; none currently replaces a hand-written equivalent in the consumers. Streaming request bodies are not necessarily cloneable or retryable. |

### Behavioral evidence

The initial repair shares a lazily initialized HTTP client within each registry.
`network_bridge_reuses_connections_and_recovers_after_decode_errors` exercises
the public network-image bridge against a real keep-alive loopback server,
starting with invalid image bytes and then decoding the committed PNG twice.
Rows cover an owned background runtime, explicit runtime injection, configured
HTTP client configuration across builder capacity transitions, separate registry
pools, and ambient runtime replacement. It checks
the actual number of accepted TCP connections, rather than private client fields.

With a fresh cell restored per request in production, the owned and injected
runtime rows fail with three TCP connections instead of one. Exact fixed source
bytes were restored in a `finally` path; no build overlapped the mutation.

`cargo nextest run -p flui-assets --features full` passes all 12 tests.
`cargo clippy -p flui-assets --all-targets --features full --locked -- -D warnings`
passes. The public loader table also covers strict UTF-8/BOM preservation,
non-success statuses including 3xx, truncated bodies, configured body timeout,
redirect policy and recovery. These tests execute real HTTP on Windows, rather
than testing an imitation of the loader predicate.

`NetworkLoader::new` now returns `reqwest::Result`; `Default` is removed rather
than converting environmental initialization errors to panics. The registry
maps initialization failure to its existing asset error boundary and retries
through Tokio's fallible lazy initialization. Hosts provide a fresh client
builder through `with_network_client`, returning configuration errors before
registry construction without changing runtime selection. TLS/resolver initialization failure
itself has not been forced on this host; that branch's evidence is source review
and the fallible API, not fault-injection execution.

`cargo tree -p flui-assets --features network -e features -i reqwest --locked`
shows only Rustls and its internal features. The facade's development graph adds
JSON. No consumer requests `stream` after replacing the provider's stream adapter
with `Response::chunk()`. Cargo regenerated the lockfile and removed the
`wasm-streams` package and reqwest's Stream-only dependency edges. No external
crate's version was changed, and `Cargo.lock` was not edited by hand.

The provider example's two tests passed on a real loopback HTTP/SSE transport.
The registry pool also reconnects after the runtime that drove its old keep-alive
connection has shut down. The widget consumer revealed a separate ownership
error: URL-only identity shared responses across registries with different HTTP
clients. Network keys now include a weak registry allocation identity (ADR-0118),
covering both decoded cache and pending work without retaining the owner. Two
real PNG responses with distinct dimensions verify isolation; same-registry
subscribers still share work and a retained key does not retain the registry.
The old implementation returned two 5x3 images instead of 5x3 and 4x2.

Restoring URL-only equality and hashing makes both the cached and pending rows
fail. Restoring per-request clients in the provider makes the accepted connection
count increase from one to three. Exact fixed bytes were restored after each
control. An independent read-only review also found that a preused configured
client could import a connection driven by a live but undriven current-thread
runtime. A real prewarmed connection timed out when reused on the bridge's owned
runtime. Registry configuration therefore accepts `ClientBuilder` rather than a
used loader, excluding imported pools by type. Its public test covers invalid
configuration and construction under an entered, undriven runtime followed by
successful worker loading. Widget fixtures are local to their package.

The narrow matrix passed: assets with no optional features (6 tests), network
(8), images (9), and full (12); widget image consumers plus unit families under
network-images (12); provider example (2). Assets doctests executed 16 cases;
51 existing ignored cases remain unexecuted. Combined assets/full and
widgets/network-images all-target Clippy passed with warnings denied, as did
facade example Clippy with its independent development feature graph. Package
listing confirms the widget fixture and shared table runner are included.

Strict source checks passed, including dependency topology, documentation links,
feature metadata, formatting and the changelog. The broad workspace run passed
650 tests with 10 configured skips. Strict private documentation passed;
workspace doctests passed 619 cases, with 389 existing ignored examples.
Windows and macOS platform/tooling Clippy passed, as did WASM Clippy and the
WASM facade's hot-reload configuration. `cargo xtask check-changed` finished
successfully. Android/iOS targets were unavailable; the iOS runner also requires
macOS. The Linux display-dependent platform suite was skipped on Windows.
These skips do not establish native platform execution. Strict private
documentation also passed with `flui-assets/full,flui-widgets/network-images`.
Both `cargo hack clippy -p flui-assets -p flui-widgets --locked --each-feature
--keep-going` passes passed with warnings denied: libraries first, then
`--tests --benches --examples`, 14 configurations per pass. This includes
no-default, all-feature and each individual feature builds without relying on
workspace feature unification. The reqwest library audit is complete.

The exact dependency-policy commands used by `cargo xtask deps` passed:
`cargo deny --workspace --locked check bans licenses sources`,
`cargo shear --locked`, and
`cargo deny --workspace --locked check advisories`.

### Consumer coverage ledger

| Consumer | Read | Compiled | Executed |
|---|---|---|---|
| Asset loader/registry | All reqwest references and architecture | No optional features, network, images and full; all targets with full | 6/8/9/12 tests respectively, including public HTTP recovery and runtime/connection families with full |
| Widget network images | Provider, key, decoded cache/pending work, architecture and image tests | network-images all targets; default workspace and WASM | 12 image/unit family tests with network-images; default workspace consumers |
| Facade AI streaming example | Provider configuration, authentication and SSE framing | Its own development feature graph, all-target workspace Clippy and separate example Clippy | 2 tests, including real local HTTP pooling/recovery; no external credentials used |
| Asset network example | Complete example | All-target full-feature Clippy | Its external HTTPS request was not run |

Loopback HTTP executes transport behavior on Windows. TLS/resolver construction
errors were not fault-injected, and no external TLS request was made. Package
listing verifies contents, not an extracted-package build. Ignored doctests,
interactive examples and native macOS/mobile execution remain unverified.

### Sources

- <https://docs.rs/reqwest/0.13.5/reqwest/>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.Client.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.RequestBuilder.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.Error.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/redirect/struct.Policy.html>
- <https://docs.rs/reqwest/0.13.5/reqwest/retry/>
- <https://docs.rs/reqwest/0.13.5/reqwest/tls/>
- <https://docs.rs/reqwest/0.13.5/reqwest/struct.Proxy.html>
- <https://docs.rs/crate/reqwest/0.13.5/source/Cargo.toml.orig>
