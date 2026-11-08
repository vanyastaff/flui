# ADR-0280: HTTP is a capability of the host

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Supersedes:** [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md) §5 (the
  network load's reactor check), and §1 only for the shape of what a load is given (lanes alone);
  [ADR-0118](ADR-0118-network-image-registry-identity.md), the registry's own HTTP client and the
  registry as the authority of a response only (§5–§6); [ADR-0107](ADR-0107-asset-byte-sources-and-decoding.md),
  its network bridge only: `NetworkLoader`, `reqwest` and the `network` and `network-images`
  features (§5).
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
       Protocol { detail }, Cancelled, Refused(SpawnError), Host { detail },
   }
   ```

   The marker and the handle live with the contract (ADR-0152 §2 item 10): `flui-assets` names
   `Http` and cannot reach `flui-view`. The handle is `Send`, unlike the clipboard's, because a
   request runs on a background lane; its answer reaches the owner thread through whatever carried
   the work there. `Method`, `HeaderName`, `HeaderValue`, `Headers` and `StatusCode` are the
   contract's own types. `ResponseBody` reads in chunks and enforces the request's limit for every
   client. Every provider builds a response head through the shared `HeadBuilder`: at most 1024
   headers, the same `ByteString` decoding where the host exposes one, and `Protocol` on a violation.
   The blocking exchange and the web exchange also share the declared `Content-Length` preflight;
   malformed or absent lengths remain subject to the streaming body limit.
2. **The client drives its requests.** `send`'s future and a body's chunks need no runtime of the
   caller's; dropping either abandons the request.
3. **Providers.** `flui-app` registers the built-in provider behind its `http` feature (on by
   default): every host sends through a stack of its own, which needs no Tokio, so an embedder's
   executors send as the host's own lanes do. The caller's future waits on a channel, and each
   exchange holds a slot of the run's IO lane until its body ends, fails or is dropped; a lane that
   shuts down under an exchange ends it with `Cancelled`. On the web the page's `fetch` sends
   (`flui_native::http::FetchClient`, a task of the page answering through a channel), and each
   exchange holds a slot of the IO lane from the request until its body ends or is dropped, so the
   lane admits and refuses it as on a native host. Android sends through the system's
   `HttpURLConnection` over JNI (`flui_native::http::UrlConnectionClient`), held on the IO lane
   the same way: each request runs on a thread of its own attached to the VM, which answers its
   head through a channel and reads the body a chunk at a time into a queue of four, waiting for
   room while the reader holds it; a dropped request or body, and a deadline kept by one thread
   per client, disconnect the connection from another thread, which ends the blocked read. Only
   system classes are called and nothing calls back into Rust, so the provider needs no class of
   FLUI's dex. A Java exception is mapped by its class (TLS, socket timeout, failed lookup or
   connection, malformed URL), else by the stage that threw it, and a failure of the bridge itself
   is `HttpError::Host`; no JNI failure panics. Windows sends through WinHTTP
   (`flui_native::http::WinHttpClient`) in the same request-per-thread model, held on the IO lane
   the same way, so an embedder's executors on Windows send too. WinHTTP runs in asynchronous
   mode: the request's thread starts each operation and waits for the status callback to complete
   it, so an abort closes the request handle only while no call is inside WinHTTP on it, never
   leaving a call to run on a closed handle whose value may name another request's. The session
   takes the system's automatic proxy and trust store; an error code maps by its meaning, a failed
   secure check naming what failed, else by the stage that failed. macOS and iOS send through an
   ephemeral `NSURLSession` of the client's own (`flui_native::http::UrlSessionClient`), on the
   same request-per-thread model and held on the IO lane the same way: the session's delegate
   hands each task's response, data and end to the request's thread, a body buffered past 256 KiB
   suspends the task until the reader drains it below 64 KiB, and a dropped request or body or a
   passed deadline cancels the task. An `NSError` in `NSURLErrorDomain` is mapped by its code
   (TLS, timeout, failed lookup or connection, bad URL); the delegate's methods are OS entries
   (ADR-0151 §16). Linux and the BSDs, which have no system stack, send through `ureq`
   (`flui_native::http::UreqClient`, `flui-native`'s `http` feature, which `flui-app`'s turns on)
   in the same model, held on the IO lane the same way, over rustls with the platform's verifier
   and the proxy the environment names. `ureq` interrupts an agent, not a request:
   `Agent::shutdown` closes the agent's connection under a blocked handshake, send or read, so
   each request runs on an agent of its own, and that shutdown is its abort. An agent whose body
   ended goes back to a pool of eight idle agents, matched by origin, so the next request there
   reuses its connection; the abort and the return each take the agent out of the request's slot
   under one lock, so an abort that comes after the return reaches no later request, and an
   aborted agent never goes back. Without the feature the
   capability is not registered.
4. **The kit's client.** `flui_platform::testing::ScriptedHttp` answers what a test scripted per
   URL, holds and releases requests, counts them and the abandoned ones, and fails an unscripted URL
   with `Connect`. `HeadlessHost` registers it among its built-ins (`HeadlessHost::http`).
5. **Images.** `AssetRegistry::load_image` and `load_network_image` take `LoadServices` (lanes and
   an HTTP client, a builder), `flui-assets` drops `reqwest`, its `network` feature and
   `NetworkLoader`, and `flui-widgets` drops `network-images`. `Image` acquires `HttpCapability` in
   `init_state` beside `BackgroundCapability` and hands both to its provider as `ImageServices`.
   A load without a client answers `AssetError::HttpUnavailable`, a failed request `AssetError::Http`
   and a non-2xx status `AssetError::HttpStatus`.
6. **The response authority is typed.** A load no longer reaches its client through the registry,
   so the registry is no longer what ADR-0118 made the authority of a response. `HttpClient` has a
   required `authority(&self) -> HttpAuthority`: an opaque value built with
   `HttpAuthority::of(&Arc<_>)` from the state the client's responses come from (the built-in
   provider's pool of the run, `ScriptedHttp`'s script), compared by that allocation and keeping it
   reserved through a weak reference, so a dead client's authority never matches a later one.
   `ImageSource::Network` carries the URL with the authority of the client that fetches it
   (`ImageSource::network(url, &http)`), and decoded images and loads in flight are kept per
   registry, URL and authority. `ImageProvider::cached` takes the `ImageServices`, so
   `NetworkImage` probes with the client its `Image` acquired; its `ImageCacheKey` keeps registry
   and URL, since one `Image`'s client does not change while it is mounted. A required method
   without a default is deliberate: a default could not name a custom client's state, and every
   such client would share images with every other.

### Consequences

- Good, because `Image::network` loads on every host with a client, the headless host included,
  and no test opens a socket.
- Good, because `flui-assets` links no TLS stack, and an application without network drops every
  HTTP stack, `ureq` and its TLS stack on Linux included, with one feature.
- Bad, because each host's provider is code of its own over a system API, and WinHTTP, the JNI
  exchange and `NSURLSession` are type-checked until a device runs them.
- Neutral, because each provider owns its own connection pool (`ureq`'s is a pool of idle agents),
  so two runs do not share connections.

### Confirmation

- `requests_on_the_runs_lanes_answer_without_a_runtime_of_the_callers` and
  `dropping_a_request_or_its_body_closes_the_connection` (`flui-app`): the built-in provider against
  a loopback server, polled without a runtime, through `ureq` held on the IO lane on Linux, an
  embedder's executors sending too, a lane shut down under a body ending it, and refusal.
- `an_agent_goes_back_to_the_pool_only_when_its_body_ended_unaborted`,
  `a_request_carries_its_parts_and_any_status_answers` and
  `ureq_errors_map_to_what_the_caller_can_act_on` (`flui-native`, Linux): the `ureq` stack's pool
  against a late abort, what a request carries and answers, and its error mapping.
- `a_request_through_fetch_answers_its_head_and_body`,
  `a_request_left_or_past_its_deadline_aborts_the_fetch` and
  `the_pages_fetch_answers_from_the_test_server` (`flui-native`, headless Chrome): the web's client;
  `a_held_exchange_takes_an_io_slot_until_its_body_lets_go` (`flui-app`): its IO slot, which
  Android's, Windows' and Apple's clients hold too.
- `a_request_on_its_own_thread_answers_its_head_and_body`,
  `a_body_left_unread_holds_the_download_until_it_is_dropped` and
  `a_request_left_or_past_its_deadline_aborts_its_connection` (`flui-native`, Linux): Android's
  request-per-thread model over a scripted stack, which Windows, macOS and iOS share; the JNI
  exchange and the `NSURLSession` delegate are type-checked, not yet run on a device.
- `an_abort_inside_a_call_closes_the_handle_as_it_returns`,
  `an_abort_while_waiting_closes_the_handle_and_ends_the_wait`,
  `a_completion_ends_the_wait_even_from_inside_the_start`,
  `winhttp_errors_map_to_what_the_caller_can_act_on`, `headers_keep_their_bytes_both_ways` and
  `the_object_name_drops_the_fragment` (`flui-native`, Linux): WinHTTP's gate between the request's
  thread, the status callback and an abort, and its header, URL and error translation; the WinHTTP
  exchange is type-checked, not yet run on Windows.
- `network_loads_fetch_with_the_callers_client_and_recover_after_errors` and
  `an_image_request_lives_exactly_as_long_as_someone_waits_for_it` (`flui-assets`).
- `network_image_loads_with_the_hosts_http_client` (`flui-widgets`): `Image::network` on the
  headless host.
- `an_authority_is_its_states_allocation_and_outlives_it` (`flui-platform`) and
  `one_registry_keeps_two_clients_images_of_one_url_apart` (`flui-assets`): §6.
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

Every host sends through a stack of its own (NSURLSession on Apple, WinHTTP on Windows,
`HttpURLConnection` on Android, `fetch` on the web, `ureq` on Linux and the BSDs). A build without
the `http` feature leaves the capability unregistered; `HttpError` describes a failure of an
existing client, not the absence of a provider. OkHttp or Cronet on Android would be a later
provider behind the same trait.
