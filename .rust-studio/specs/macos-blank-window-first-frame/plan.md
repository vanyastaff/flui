# Plan — the blank "white window" on desktop launch, and the first-frame race behind it

Status: **cause established by measurement and photographed (2026-09-20, real Mac).** The recorded
observation — a bundle launched through the UI automation service showed a white window while the
same bundle rendered when started directly — is not a launch-route difference. It is the **window
being ordered front before its first frame is presented**: for as long as the first frame takes, the
window is on screen filled with its own background, which on this host measures `rgb(240,240,240)`,
and that is what a viewer sees. The two observations differ by *warmth* (a cold first frame is
seconds, a warm one is sub-frame), not by route. The fix is to defer the first show until the
compositor has presented a frame; it is **planned here and not yet implemented** (§7), and §8 states
what that leaves unverified.

This task's predecessor is `.rust-studio/specs/macos-launchservices-render-gate/plan.md`, whose §6
left "the original observation's cause" explicitly unattributed. §3 and §5 below attribute it.

## 1. Task and where it comes from

`docs/BETA.md:364-369`, in the beta-preparation report:

> Launching the resulting bundle through the UI automation service showed a white window; directly
> launching the same bundled executable rendered the counter, and two observed pointer clicks changed
> 18 to 19 to 20.

The recorded sentence invites a route explanation ("the automation service launches differently"),
and the predecessor task tested exactly that: three launch routes (`direct`, `open`, `open -g`) ×
multiple launches, **15/15 rendered** on the in-repo fixture and **9/9** on a CLI-built counter
bundle. Routes were therefore the wrong axis. The right axis is *when the frame arrives*.

## 2. What the window is

Measured on this host — one display, `id=2`, `asleep=0`, 3440×1440 points = 3440×1440 px (scale 1):

| property | value | how |
|---|---|---|
| bounds | `800×632` at `1320,202`, centred | `CGWindowListCopyWindowInfo`, window's own number |
| 632 = 600 + 32 | 800×600 content + a 32 px title bar | `AppConfig` default 800×600 (`crates/flui-app/src/app/config.rs:231`) |
| centring | `x=1320` for a 800-wide window on 3440 | `-[NSWindow center]`, `crates/flui-platform/src/platforms/macos/window.rs:364` |
| **fill, drawn window** | **`rgb(240,240,240)` 98.70 %, ink 1.30 %** | whole-screen capture censused inside the window rect |
| **fill, window with nothing drawn** | **`rgb(240,240,240)` 99.78 %, ink 0.22 %** | the blank control, §5 |

The two rows differ by **1.08 percentage points of ink on the same field**. A blank window and a
rendered window are the same near-white rectangle; only the content distinguishes them. The counter
renders correctly and centred: content centre `y≈330` in a body spanning `32..632` (centre 332) —
the `MainAxisAlignment::Center` fix from `3f98e0e9` holds on the macOS desktop path.

## 3. The mechanism, in code

| step | site |
|---|---|
| window options built | `crates/flui-app/src/app/runner/main_window.rs:227` |
| **window opened → ordered front** | `main_window.rs:228` → `MacOSPlatform::open_window` → `makeKeyAndOrderFront:` at `platforms/macos/window.rs:359-360`, guarded by `options.visible` |
| `visible` is hardcoded true | `crates/flui-app/src/app/config.rs:384` |
| installer, then the whole GPU stack | `crates/flui-app/src/app/runner/desktop.rs:49`; `Instance::new` `crates/flui-engine/src/renderer.rs:1133`, `request_adapter` `:1163`, `request_device` `:1179` |
| **first frame presented** | `crates/flui-engine/src/renderer.rs:2115` `self.queue.present(output)` |

Everything in rows 4–5 happens **after** the window is already composited on screen. Between the two
there is no signal: no first-frame callback, no flag, nothing that could defer the show — the only
first-frame latch (`FrameClock::first_frame_sent`, written at `crates/flui-app/src/app/ui_realm.rs:2562`)
latches at the *produce* stage, before the submit, and never reaches the embedder. `WindowOptions.visible`
exists and macOS honours it, but no app-level path can currently decline it, and there is no
post-creation `hide`/`set_visible` — only `PlatformWindow::show()` (`crates/flui-platform/src/traits/window.rs:375`,
macOS impl `platforms/macos/window.rs:1018`).

Windows, Linux (winit) and macOS all show at creation (`platforms/windows/window.rs:313-322`,
`platforms/linux/mod.rs:108`), so this is a cross-backend shape, measured here on macOS only.

## 4. Why "white", and why the two observations differ

**White.** The window is filled with its own background before anything is painted, and AppKit's
light-mode window background and the app's surface colour are both ≈`(240,240,240)`. On this host the
unpainted window measures 99.78 % of exactly that colour (§5). Nothing about it is white *paint*; it
is an absent frame on a light field.

**The difference between the two observations is warmth, not route:**

| launch | window appears | undrawn for | how measured |
|---|---|---|---|
| cold (first launch of that build) | 2.21 s | **2.81 s** | window-scoped capture: window exists, no backing store |
| warm (every launch since) | 3.10 s | **0 s** — already drawn at first sighting | whole-screen census, `blank-phase.py` |

A service that photographs on a fixed short delay in the first case, and a person double-clicking a
bundle that has been launched before in the second, is exactly the reported contrast.

**Two corrections to readings taken before this one, both of which had pointed the wrong way:**

- **`screencapture -l` on a window with no backing store returns a flat dark image** (measured: 1
  bucket, `(32,32,32)`). That is a capture artifact, pixel-identical to a genuinely black window. The
  earlier note that "the undrawn fill is black" was reading the artifact; the on-screen fill is
  `(240,240,240)`. Only a whole-screen capture can see an unpainted window.
- **A mostly-black whole-screen capture is not evidence the display was asleep.** An earlier reading
  attributed black frames to display sleep; the display reported `asleep=0` and the black was a
  full-screen terminal behind the sampled rect. The display's own power state (`CGDisplayIsAsleep`)
  is now the assertion, not pixel darkness.

## 5. The blank control

Racing a cold launch is not reproducible on demand (the caches that made it slow are warm now), so
the state was built directly: an `NSWindow` with the same style mask and size, ordered front and
centred, with nothing drawn into its content view.

| sample | buckets | dominant | ink |
|---|---|---|---|
| 0–4 (5 samples, stable) | 54 | `rgb(240,240,240)` **99.78 %** | 0.22 % |

The residual 0.22 % is the title bar's own text and traffic lights — chrome, not content. Photographed
whole-screen and censused in the window rect; the image is at `/tmp/flui-white/blank-control/control.png`.

This is the window a viewer sees during the gap, and it is indistinguishable from the application's
first frames except by the missing content.

## 6. The gate this exposes

`scripts/check-macos-launch-render.py:307→317` captures **once**, immediately after a window appears.
That is strict by construction — and it means the gate races the first frame: under a cold launch it
would capture the flat window and report **FAIL** for a bundle that renders 2.8 s later. Its 15/15 and
9/9 passes were all warm. This is a defect in the gate's *timing*, not in its oracle: "never draws" and
"has not drawn yet" are currently the same reading.

## 7. The plan

1. **Defer the first show to the first presented frame.** Open the window with `visible: false` and
   call `PlatformWindow::show()` once a frame has been presented. The frame count already exists —
   `UiRealm::frames_rendered()` counts presented frames (`ui_realm.rs:2856`, on the `Presented` arm) —
   so the hook needs no new signal, only a caller.
2. **A failsafe, because an invisible window is worse than a blank one.** If the first frame fails
   (device loss, unsupported surface), the window must still be shown: show on the first frame-drive
   iteration that reports an error or a withheld outcome, and on a bounded deadline. A GPU failure
   must not present as an application that never appears.
3. **Keep the scope honest.** The change is in the shared desktop path, so it reaches Windows and
   Linux too; only macOS is measurable on this host, and §8 says so.
4. **Teach the gate the difference.** Bound the settle: retry the capture until the oracle passes or a
   deadline expires, and report the time to first frame. A blank window then fails as "never drew
   within N s" instead of racing, and the latency becomes a recorded number rather than an
   intermittent FAIL.

## 8. What remains unverified

- **The fix is not implemented.** Everything above is the measurement; §7 is a plan.
- **The cold gap is a single measurement** (2.81 s, one cold launch). The caches that produced it are
  per-user and warm; it was not reproduced on demand, and the 2.81 s is that launch's number, not a
  distribution.
- **The blank control is a hand-built AppKit window**, not flui's own window in its undrawn state. It
  is the same construction (`NSWindow` + `makeKeyAndOrderFront` + `center`, nothing drawn), which is
  why it stands in; it is not flui's object.
- **Windows and Linux are unmeasured.** They show at creation too, so the shape is shared, but no
  claim is made about their blank interval.
- **The 18 → 19 → 20 click observation remains as recorded.** It is not re-tested here; the note that
  the initial value was already 18 still stands.
