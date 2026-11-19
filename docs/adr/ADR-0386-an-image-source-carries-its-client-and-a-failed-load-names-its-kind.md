# ADR-0386: An image source carries its client, and a failed load names its kind

- **Status:** Accepted (2026-11-19)
- **Date:** 2026-11-19
- **Supersedes:** [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md) §5 for the shape of an
  image load (`load_image`, `load_network_image`, an HTTP client in `LoadServices`) and its errors
  (`AssetError::HttpUnavailable`, `Http`, `HttpStatus`), and §6 for how a network source is built
- **Related:** [ADR-0385](ADR-0385-flui-assets-is-the-decoded-image-cache.md),
  [ADR-0250](ADR-0250-asset-registry-keeps-decoded-images.md),
  [ADR-0279](ADR-0279-image-loads-run-on-the-lanes-their-caller-holds.md),
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
  [ADR-0325](ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md)

## Context and problem statement

`ImageCache` took a request in two shapes: `load_image(path, services)` and
`load_network_image(url, services)` took strings, `cached_image(&ImageSource)` took a source. A
network source's cache key is its URL with the authority of the client that fetches it, and two
paths built it: `ImageSource::network(url, &http)` for the probe and, inside the cache, the URL
with whatever client `LoadServices` held for the load. A widget hit the cache only if it built
the probe from the same client it gave the load, and nothing checked that it did.

A failure was `AssetError`, whose variants carried strings: a missing file, a path that is not a
regular file and a file over the size limit were all `LoadFailed { path, reason }`, so
`flui-widgets` could not tell "not found" apart, and a decode failure arrived as `LoadFailed`
too. `Cache`, `Bundle`, `UnsupportedFormat` and `InvalidData` had no producer.

A panic in the client or the file read was answered by `Completion`'s `Drop`, which checked
`std::thread::panicking()`. That holds only while the executor drops the run during the unwind.
An executor that catches the panic and drops the task later gave `false`, so the waiters took the
load over and called the panicking client again, up to the handover limit, and answered "dropped
unfinished" instead of the panic.

## Decision drivers

- One request shape: what the probe looks up is what the load fetches.
- A cache key holds everything the result depends on (AGENTS.md, "Before each push" item 5).
- A failure the caller acts on is a type, not a string.
- User code is contained at the point it runs (ADR-0281, ADR-0287), not inferred afterwards.
- A description type's builder has the bare name (ADR-0325).

## Considered options

1. **The source carries the client that reads it**; `LoadServices` carries only lanes.
2. Keep the client in `LoadServices` and refuse a load whose client's authority differs from
   the source's.
3. Keep the two load methods and derive the probe's source from `LoadServices` too.

## Decision

Chosen option: 1, because a source that holds its reader cannot be loaded with another one, so
no load can be refused for a mismatch.

```rust
impl ImageSource {
    pub fn asset(path: impl Into<Arc<str>>) -> Self;
    pub fn network(url: impl Into<Arc<str>>, http: &Http) -> Self;
    pub fn path(&self) -> Option<&str>;
    pub fn url(&self) -> Option<&str>;
}                                   // Eq and Hash by path, or by URL and the client's authority
impl LoadServices { pub fn new() -> Self; pub fn lanes(self, lanes: Arc<dyn Executor>) -> Self; }
impl ImageCache {
    pub fn load(&self, source: &ImageSource, services: &LoadServices) -> ImageLoad;
    pub fn get(&self, source: &ImageSource) -> Option<Image>;
}
pub struct ImageLoadError;          // image() -> &ImageSource, kind() -> &ImageLoadErrorKind
#[non_exhaustive]
pub enum ImageLoadErrorKind {
    NotFound, NotAFile, TooLarge { limit: u64 }, Io(Arc<io::Error>), Decode(ImageDecodeError),
    Http(HttpError), HttpStatus(StatusCode), LanesRefused(SpawnError),
    Panicked { message: Option<Arc<str>> }, Abandoned { handovers: u8 },
}
```

1. **The key holds no client.** The cache keeps images and loads in flight under the path, or the
   URL and the client's `HttpAuthority`; the `Http` handle lives in the source a request holds,
   so a kept image keeps no client alive (ADR-0118).
2. **No client, no network source.** A caller without a client cannot build a network source;
   `NetworkImage` answers `ImageProviderError::HttpUnavailable` itself and sends nothing.
3. **Every waiter gets the same kind, with its source.** A load's outcome is shared; each
   `ImageLoad` wraps a failure with its own source. The kind's message includes the error it
   carries, so neither type has an error `source()`.
4. **A panic is contained where it happens.** Each poll of the read or the request runs inside
   `FirstPanic::contain`, and so does the decode; a panic answers `Panicked` with its message, the
   ended future is dropped under containment, and the payload is discarded (ADR-0281). The
   `panicking()` check in `Completion`'s `Drop` is left for a panic in the cache's own code.
5. **Mapping in the widgets.** `NotFound` is `ImageProviderError::SourceNotFound`, `Decode` is
   `DecodeFailed`, every other kind is `AssetLoadFailed` with the error's message.

### Confirmation

- `a_client_that_panics` (`flui-assets`, in
  `network_loads_fetch_with_the_callers_client_and_recover_after_errors`): a client that panics
  is called once, and both waiters answer `Panicked` with its message; without the containment
  the panic leaves `run_until_idle`.
- `a_missing_file_is_an_error_and_the_next_load_succeeds`, `a_path_that_is_not_a_regular_file_is_refused`,
  `an_oversized_file_is_refused_before_it_is_read` and `decoded_pixels_are_bounded_before_decoding`
  match on `NotFound`, `NotAFile`, `TooLarge` and `Decode(TooLarge)`.
- `an_image_missing_a_service_answers_which_one` (`flui-widgets`).

## Pros and cons of the options

### 1. The source carries its client

- Good, because the probe and the load cannot disagree on the client.
- Good, because a load without a client is not a state the cache has to answer.
- Bad, because a request holds an `Http` clone for as long as it lives, and so does an error
  that names its source.

### 2. Refuse a mismatched client

- Good, because `LoadServices` keeps everything a load runs with in one place.
- Bad, because the mistake is found when the load runs, not when it is written, and needs an
  error kind of its own.

### 3. Derive both from `LoadServices`

- Good, because the call sites change least.
- Bad, because `cached_image` would need the services too, and a source would still be two
  values that must agree.
