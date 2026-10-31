# ADR-0280: HTTP is a capability of the host

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Supersedes:** [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md) §5 (the
  network load's reactor check), and §1 only for the shape of what a load is given (lanes alone).
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (the seam the capability travels
  through), [ADR-0047](ADR-0047-unified-execution-services.md) (no runtime outside the lanes),
  [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (the scripted client lives next to its
  contract), [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §2 (no `http` or `reqwest`
  type in a contract signature)

## Context and problem statement

`flui-assets` fetched network images with its own `reqwest` client. After ADR-0279 the fetch runs
on the IO lane of whoever asks, and `reqwest` needs a Tokio reactor wherever its request is
polled: a native host's default lanes have one, the headless host's deterministic lanes, an
embedder's executors and a load with no lanes do not, so `Image::network` failed there
(`AssetError::ReactorUnavailable`). Tests of network images either opened loopback servers or could
not run on the headless host at all, and a tier-S asset crate linked a TLS stack. The question is
where HTTP lives so that a widget, the asset registry and a test reach the same client, the request
runs on the host's lanes, and no test reaches the network.

## Decision drivers

- One client per host, chosen by the composition root, reached through the capability seam like the
  clipboard and storage (ADR-0152).
- The caller's future needs no async runtime: the client drives its own requests.
- No thread or runtime outside the UI runtime's lanes (ADR-0047).
- A host whose FLUI has no client yet fails loudly with a typed gap, never a panic and never a
  skipped scenario.
- Tests script responses through the kit and never open a socket.
- The contract names no upstream HTTP type (ADR-0089 §2) and grows (streaming, progress, caching)
  without breaking callers.

## Considered options

1. A capability in the contract crate: an `HttpClient` trait whose future needs no runtime of the
   caller's, a `Send` handle `Http`, providers per host, and a scripted client in the kit.
2. Keep `reqwest` in `flui-assets` and require a reactor on every host's IO lane.
3. A blocking client (`ureq`) run on the compute lane.

## Decision

Chosen option: 1, because the host already owns its lanes and network stack, and only a capability
lets a test hand every layer the same scripted client.

1. **The contract is in `flui-platform` (`flui_platform::http`).**

   ```rust
   pub trait HttpClient: Send + Sync + 'static {
       fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse>;
   }
   pub type HttpFuture<T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + Send + 'static>>;
   #[non_exhaustive] pub enum HttpCapability {}            // Handle = Http, NAME = "http"
   #[derive(Clone)] pub struct Http { /* Arc<dyn HttpClient> */ }
   pub struct HttpRequest { /* method, url, headers, body, body limit, deadline; builder */ }
   pub struct HttpResponse { /* StatusCode, Headers, ResponseBody */ }
   pub trait BodySource: Send + 'static { fn poll_chunk(..) -> Poll<Option<Result<Vec<u8>, HttpError>>>; }
   #[non_exhaustive] pub enum HttpError {
       InvalidUrl { url }, Connect { detail }, Tls { detail }, Timeout, TooLarge { limit },
       Protocol { detail }, Cancelled, Refused(SpawnError), NotYetOnThisHost { host: HttpHost },
   }
   ```

   The marker and the handle live with the contract (ADR-0152 §2 item 10): `flui-assets` names
   `Http` and cannot reach `flui-view`. The handle is `Send`, unlike the clipboard's, because a
   request runs on a background lane; its answer reaches the owner thread through whatever carried
   the work there. `Method`, `HeaderName`, `HeaderValue`, `Headers` and `StatusCode` are the
   contract's own types. `ResponseBody` reads in chunks and enforces the request's limit for every
   client.
2. **The client drives its requests.** `send`'s future and a body's chunks need no runtime of the
   caller's; dropping either abandons the request.
3. **Providers.** `flui-app` registers the built-in provider behind its `http` feature (on by
   default): `reqwest` on the run's IO lane, one connection pool per run. The caller's future waits
   on a channel; the exchange on the lane stops when the channel closes. On lanes an embedder
   injects, which may run no Tokio drivers, it spawns nothing and answers
   `NotYetOnThisHost { host: EmbedderExecutors }`; Android and the
   web answer `NotYetOnThisHost` until their providers land (`docs/gaps.md`). Without the feature
   the capability is not registered.
4. **The kit's client.** `flui_platform::testing::ScriptedHttp` answers what a test scripted per
   URL, holds and releases requests, counts them and the abandoned ones, and fails an unscripted URL
   with `Connect`. `HeadlessHost` registers it among its built-ins (`HeadlessHost::http`).
5. **Images.** `AssetRegistry::load_image` and `load_network_image` take `LoadServices` (lanes and
   an HTTP client, a builder), `flui-assets` drops `reqwest`, its `network` feature and
   `NetworkLoader`, and `flui-widgets` drops `network-images`. `Image` acquires `HttpCapability` in
   `init_state` beside `BackgroundCapability` and hands both to its provider as `ImageServices`.
   A load without a client answers `AssetError::HttpUnavailable`, a failed request `AssetError::Http`
   and a non-2xx status `AssetError::HttpStatus`.

### Consequences

- Good, because `Image::network` loads on every host with a client, the headless host included,
  and no test opens a socket.
- Good, because `flui-assets` links no TLS stack, and an application without network can drop
  `reqwest` with one feature.
- Bad, because Android, the web and embedders' executors have no client yet: their network images
  fail with a typed gap until the system stacks land.
- Neutral, because each provider owns its own connection pool, so two runs do not share
  connections.

### Confirmation

- `requests_on_the_runs_lanes_answer_without_a_runtime_of_the_callers` and
  `dropping_a_request_or_its_body_closes_the_connection` (`flui-app`): the built-in provider against
  a loopback server, polled without a runtime, the embedder-executor gap and refusal.
- `network_loads_fetch_with_the_callers_client_and_recover_after_errors` and
  `an_image_request_lives_exactly_as_long_as_someone_waits_for_it` (`flui-assets`).
- `network_image_loads_with_the_hosts_http_client` (`flui-widgets`): `Image::network` on the
  headless host.
- `cargo xtask send-positions` lists the contract's `Send` and `Sync` positions.

## Pros and cons of the options

### 1. A capability in the contract crate

- Good, because every layer reaches one client, and a test replaces it once.
- Good, because the host picks a stack per platform behind one trait.
- Bad, because each platform needs a provider before its network images work.

### 2. Keep `reqwest` in `flui-assets` with a reactor on every lane

- Good, because it changes nothing in the asset crate.
- Bad, because it freezes Tokio into the lane contract (ADR-0047), keeps the headless host without
  network images, and leaves tests on loopback servers.

### 3. A blocking client on the compute lane

- Good, because it needs no reactor.
- Bad, because a slow server holds a compute thread for its whole deadline, and the web has no
  blocking IO.

## More information

The system stacks (NSURLSession on Apple, WinHTTP on Windows, OkHttp or Cronet on Android, `fetch`
on the web) are later providers behind the same trait; each removes a `NotYetOnThisHost` host or
replaces `reqwest` where the system's proxy and trust settings matter.
