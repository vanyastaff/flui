# ADR-0058 pacing measurements — 2026-09-05

All runs: `cargo run -p flui --example animated_box_app` (dev profile, the
same profile ADR-0029 measured), NVIDIA RTX 3070 Ti driver 595.84,
3440x1440 @ 164.89 Hz (6.065 ms period), 20 s each, traces via
`RUST_LOG=...,flui.gpu=trace,flui.pace=trace`. X11 runs raise the window once
a second (`xdotool windowactivate`) so an occlusion state cannot be mistaken
for a pacing result. Raw logs are not committed (one is 20 MB); this file is
the reduction, produced by one script over the same trace events for every
row.

"not traced" is honest, not zero: the acquire-duration and fallback-sleep
probes were added during this investigation, so the "before" rows — captured
on the unmodified build — have no samples for those columns. "n/a (no hook)"
means the pre-present hook did not exist in that build.

| run | presents | last | median | p90 | p99 | gaps >1.6 period | acquire median | acquire p90 | notifies | sleeps |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **X11, before** (ADR-0029 sleep) | 1555 | +20.0s | 16.49 ms | 19.17 ms | 23.25 ms | 1003/1428 | not traced | not traced | n/a (no hook) | not traced |
| **X11, after** (ADR-0058) | 3228 | +20.0s | 6.83 ms | 7.14 ms | 10.34 ms | 46/2976 | 12.00 us | 5782.00 us | 3228 | 0 |
| **Wayland, before** (ADR-0029 sleep) | 1298 | +11.1s | 1.69 ms | 17.48 ms | 52.68 ms | 385/1155 | not traced | not traced | n/a (no hook) | not traced |
| **Wayland, after** (ADR-0058) | 2852 | +20.0s | 6.79 ms | 8.20 ms | 14.37 ms | 169/2639 | 10.00 us | 25.00 us | 2852 | 0 |
| X11, sleep set to 0 (premise probe) | 5416 | +19.6s | 3.15 ms | 6.45 ms | 9.80 ms | 55/5082 | 15.00 us | 5679.00 us | 0 | 112844 |
| X11, notify landed + buggy `next_wake` (froze) | 2215 | +20.0s | 6.01 ms | 6.54 ms | 11.91 ms | 54/1928 | 11.00 us | 5764.00 us | 2215 | 0 |

## Presents per second — a freeze is visible here and invisible in the median

- **X11, before** (ADR-0029 sleep): `[51, 75, 91, 89, 99, 89, 94, 78, 96, 78, 69, 91, 86, 67, 67, 71, 67, 66, 65, 66]`
- **X11, after** (ADR-0058): `[101, 150, 193, 211, 214, 153, 159, 160, 158, 156, 155, 161, 157, 155, 163, 160, 150, 153, 156, 162, 1]`
- **Wayland, before** (ADR-0029 sleep): `[56, 86, 133, 136, 174, 149, 183, 106, 103, 99, 67, 6]`
- **Wayland, after** (ADR-0058): `[80, 132, 136, 144, 142, 137, 137, 160, 164, 161, 163, 160, 163, 143, 135, 127, 141, 144, 144, 139]`
- X11, sleep set to 0 (premise probe): `[170, 163, 199, 335, 363, 273, 308, 285, 304, 219, 347, 282, 392, 345, 322, 398, 199, 165, 165, 182]`
- X11, notify landed + buggy `next_wake` (froze): `[121, 165, 208, 221, 223, 215, 210, 195, 164, 163, 163, 165, 1, 0, 0, 0, 0, 0, 0, 0, 1]`

## What each row settles

- **X11, before → after.** The sleep quantized a 165 Hz panel to ~60 frames
  per second: median 16.49 ms, and 1003 of 1428 inter-present gaps longer
  than 1.6 panel periods. After: 6.83 ms median, 46 of 2976 over that bound,
  and the `Fifo` block visibly doing the pacing (acquire p90 5.78 ms against
  a 6.065 ms period).
- **Wayland, before → after.** The "before" run is bimodal — bursts at
  1-2 ms separated by 16-18 ms sleeps — and stops presenting at +11.1 s
  while the animation keeps rebuilding. After `pre_present_notify`, the
  compositor paces: 2852 presents across the full 20 s at a 6.79 ms median,
  and the window is silent when hidden rather than spinning.
- **The premise probe (sleep set to 0)** is why "the block never engages"
  was the wrong reading of the first capture. With the sleep gone the
  acquire p90 is 5.68 ms against a 6.065 ms period: the block does engage,
  behind a two-image swapchain (wgpu-hal `min_image_count(latency + 1)`)
  that the 16 ms sleep was draining before a third acquire could reach it.
  The same probe shows what a sleep-free but *ungated* loop costs: 112 844
  pipeline passes for 5416 presents.
- **The buggy-`next_wake` row** is kept because it is the evidence behind
  ADR-0058 decision 3, not a discarded attempt: `pre_present_notify` had
  landed and the pacing was correct (median 6.01 ms) right up to +12 s,
  where the loop froze for the rest of the run.

## The freeze transition, traced

The first `FallbackWake` cleared a just-passed deadline inside `next_wake`.
`about_to_wait` observed it 5 µs late, cleared it, and parked the loop in
`ControlFlow::Wait`; the redraw that its own `ResumeTimeReached` poke would
have queued never happened, and the pending-deferral rule was suppressing the
realm's own redraw echo, so nothing woke the loop again:

```
11.0230  WAKE-AHEAD 6075       about_to_wait arms WaitUntil(D)
11.0291  WAKE-DROPPED late=5   about_to_wait runs 5 µs after D, clears it -> Wait
11.0351  WAKE-NONE             idle
11.0411  WAKE-NONE             idle
15.0866  WAKE-NONE             (4 s later; nothing has produced since)
```

Hence ADR-0058 decision 3: `gate` (the frame callback) is the only consumer;
`next_wake` reports and never destroys.
