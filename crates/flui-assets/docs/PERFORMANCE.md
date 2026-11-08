# Performance and memory

The registry's performance contract is bounded sharing and retention, not a
claimed latency or throughput advantage. Measure the actual image formats,
sizes, source latency and access pattern of an application before changing its
budget or lane configuration.

## Decoded-byte retention

Each cached RGBA8 image weighs:

```text
width × height × 4 bytes
```

`with_decoded_image_budget` bounds the sum retained by one registry. The default
is 256 MiB, and 64 MiB on Android, iOS and the web. Hits and explicit cache
probes refresh recency; eviction is least recently used first. There is no idle
or lifetime expiration.

An over-budget image is returned without being cached. A zero budget disables
completed retention, but concurrent requests still share loads. The budget does
not include pixels held by callers after eviction, renderer textures, encoded
bytes or temporary decoder allocations.

Moderate memory pressure trims to half the configured budget; critical pressure
clears completed images. Neither level cancels in-flight work.

## Input and decode bounds

A regular file is checked before opening and read in 256 KiB chunks. Files and
HTTP response bodies are capped at 256 MiB, preventing unbounded encoded
buffering. Non-regular files are refused so a device or FIFO cannot block a
shared IO worker in `open`.

Before pixel production, the decoder checks dimensions with checked arithmetic.
Decoded RGBA8 output is capped at 256 MiB, or 128 MiB on Android, iOS and the
web; the same value is the decoder allocation limit. Small compressed input
therefore cannot request unbounded pixel allocation.

At most `with_decode_threads` decodes run concurrently. The default is half the
available cores, at most four, so transient decoder memory is bounded separately
from the retention budget.

## Admission and scheduling

A registry grants four load slots per decode thread in admission order. A load
holds its slot from before reading until its decode returns. This bounds:

- file reads and HTTP requests in flight;
- encoded buffers waiting for decode;
- work queued ahead of visible images.

Once bytes are ready, the earliest admitted ready load gets a decode slot. A
slow earlier fetch holds no decode slot, so it cannot leave the decoder idle
while a later file is ready. Abandoned loads release their slots and never start
a queued decode.

IO and compute are separate. Decodes never occupy the IO lane, and waiting for
compute room does not block its worker. With no lanes, work happens only while
the returned future is polled; holding unpolled no-lane loads does not consume
registry admission slots.

## Sharing versus duplication

One source in one registry has at most one active load. Subscribers share its
outcome and decoded allocation. Failures are not retained, so retries perform
new work. Separate registries intentionally duplicate caches and loads.

Network identity includes HTTP client authority. This may create separate
entries for one URL when clients differ; that is required for correctness when
they can return different bytes.

## Measuring an application

Record cold source time, decode time, warm hits, retained decoded bytes and
caller-held images separately. Include the registry configuration and image
inputs in any benchmark. A cache hit does not measure network or decode
performance, and entry count alone is not a memory measurement.

If memory remains high after lowering the budget, inspect:

1. `Image` values retained by mounted widgets or application state;
2. admitted encoded bodies and files;
3. concurrent decoder allocations;
4. renderer textures and atlases;
5. additional registries.

Do not put a mutex around the whole registry to serialize loads. Its internal
locks already protect short state transitions, while source work and callbacks
run outside them.

## References

- [Architecture and behavior tests](ARCHITECTURE.md)
- [User guide](GUIDE.md)
