# Market and dependency survey — issue #1146

Written before the builder brief, on disk rather than in the conversation. The four steps are the
fixed shape from the studio's memory protocol: the issue's own thread, then the crate's own
manifest and siblings **first**, then the reference at the pinned tag and its tracker, then the
surrounding ecosystem. Each step records a result even when the result is empty.

## 1. The issue's own comment thread

`gh api repos/vanyastaff/flui/issues/1146` reports `comments: 0`, so there is nothing to mine and
nothing was missed. The body itself, however, carries a **market-shape section naming four
sources**, and those are mined in step 4 below rather than taken on the issue's word.

**Tooling trap worth recording, because it makes an empty thread and a broken command look
identical.** `gh issue view 1146 --comments` prints **0 bytes and exits 0** in this environment
(reproduced three times), while `gh issue view 1146` prints the body normally at 2293 bytes. The
emptiness above was therefore established through `gh api`, not through the `--comments` flag. Any
future sweep on this host that concludes "the thread was empty" from `--comments` alone has
concluded nothing.

## 2. The crate's own manifest, lockfile, and siblings — first

**Where the dependency is declared, and what that implies for the decomposition.**
`crates/flui-platform/Cargo.toml:106-108` carries
`[target.'cfg(target_os = "android")'.dependencies] android-activity = { version = "0.6", features = ["native-activity"] }`
alongside `libc`. **`crates/flui-app/Cargo.toml:121` declares the identical dependency**, so the
Android runner *can* name `android-activity` types. Routing the signal through a `PlatformWindow`
callback is therefore a design choice about where the signal belongs, not a layering necessity
imposed by the manifest. The plan's decision L is corrected on this point.

**Is the locked version current?** `Cargo.lock:144-147` pins `android-activity` 0.6.1. crates.io
lists 0.6.1 as the newest of 15 published versions, released 2026-03-24, so **no version bump is
available** and the contract read out of its source is the current one. This closes the question
rather than leaving it assumed.

**Feature flags.** The crate declares four: `api-level-30`, `game-activity`, `native-activity`, and
no defaults. FLUI enables `native-activity`, which is the module whose `glue.rs` carries the
`pre_exec_cmd`/`post_exec_cmd` ordering every mechanism claim in the plan rests on. The crate's
*other* backend, `game-activity`, is a different Activity base class with its own lifecycle glue, so
the ordering verified here would need its own verification if FLUI ever switched. That is a stated
boundary, not an assumption.

**The sibling that already solved half of this.** `Renderer::recover`'s doc in
`crates/flui-engine/src/wgpu/renderer.rs:1047-1051` already says it returns
`EngineError::SurfaceTargetUnavailable` "if the window owner reports the native target is gone **or
suspended (a destroyed window, a suspended Android surface)**", and instructs the caller to treat
that as transient. `SurfaceLease::probe`'s doc (`surface_lease.rs:51`) says the same thing. So the
plan's A8 classification of a `true` that arrives with no window is **the contract this crate
already documents**, not a new invention, and the existing handlers at `flui-app/src/app/direct.rs:216`
and `runner/web.rs:265` already branch on it. `SurfaceAcquireOutcome` likewise already carries an
`Occluded` disposition, so a new suspended disposition joins an established family.

## 3. The reference at the pinned tag, and its tracker

**`.flutter` is absent from this worktree**, and `git -C .flutter describe --tags` cannot run at
all. No Flutter citation in the plan can be checked against the pinned tag on this host.

**It would not settle this one if present.** The sparse checkout AGENTS.md prescribes is
`packages/flutter/lib` plus `packages/flutter/test`, which is the *framework*. The Android surface
contract lives on the **engine** side (`FlutterSurfaceView`, `SurfaceProducer`), a different
repository. The framework reference is silent on this change **by scope**, not by omission, which is
why the market survey below is the reference that applies and why the ADR plus the `## Mapping
decisions` entry carry the accounting.

**The tracker result is the strongest argument for the before-signal shape, and it is stronger than
the plan stated.** [flutter#160933](https://github.com/flutter/flutter/issues/160933) is titled
"SurfaceProducer needs a 'will destroy' signal, not a 'did destroy'", labelled `platform-android`,
`team-engine` and `c: API break`, and closed against PR #160937. Its own words: the API "currently
provides an `onSurfaceDestroyed` callback that is invoked **after** a surface has been destroyed",
and "we need a callback that is invoked *before* the call to `cleanup()`", with the code sample
naming `onSurfaceDestroying`. Three precision fixes follow for the plan: the signal Flutter asked
for is `onSurfaceDestroying`, it precedes the engine's `cleanup()`, and it is an engine
`SurfaceProducer` API rather than the framework `onSurfaceCleanup` the plan named.

## 4. The surrounding ecosystem

**The dependency's own maintainer states this plan's central design decision.** In
[bevy#6830](https://github.com/bevyengine/bevy/pull/6830), `rib` (an `android-activity` maintainer)
writes: "The only thing that needs to be re-created is the top-level render target/surface, not all
wgpu rendering resources." That is exactly the surface-only release and recreate the plan specifies,
which keeps the instance, adapter and device, and it comes from the person who owns the lifecycle
code being read. It is the single best citation in this survey and it belongs in the ADR.

**Bevy settled on the same shape as the plan, after trying the other one first.**
[bevy#9937](https://github.com/bevyengine/bevy/pull/9937) ("Android: handle suspend / resume",
merged 2023-10-02 as `eb1effa`, milestone 0.12) originally despawned the window on suspend and
spawned a fresh one on resume. The author revised it, and the commit message reads "keep Bevy
window, only recreate Winit window and wgpu surface". [bevy#13689](https://github.com/bevyengine/bevy/pull/13689)
states the mechanism in the same terms: "on suspend we drop the `RawHandleWrapper` component but
keep the window to be able to free gpu resources and recreate the window on resume". So a mature
engine that is not FLUI, having attempted full window recreation, moved to keeping the presentation
and rebuilding only the surface.

[bevy#9057](https://github.com/bevyengine/bevy/issues/9057) (still open) names the underlying
platform fact: Android "destroys all active GL/Vulkan surfaces" when an app is suspended.

**winit 0.30 supplies a concrete second emitter for the new callback, which upgrades decision L
from a hypothetical to a scheduled one.** [winit#3786](https://github.com/rust-windowing/winit/pull/3786)
("android: Forward `suspended()` and `resumed()` events", merged October 2024) forwards
`suspended()`/`resumed()` keyed off Android's `onStop()`/`onStart()` callbacks. winit is already a
workspace dependency and FLUI already has a winit backend. So the per-window callback shape the plan
chooses has a named second emitter in this very dependency set, and it would feed the same `false`/`true`
pair.

**One divergence the survey makes visible, and it favours the plan.** winit and bevy key on
`onStop`/`onStart`, while this change keys on `Pause`/`Resume` plus `TerminateWindow`/`InitWindow`.
Releasing on both of our events is the more conservative of the two: `TerminateWindow` is the exact
moment the native handle dies, and `Pause` is the earlier, cheaper release whose only cost is one
redundant recreate on a path that wants a full repaint anyway. Releasing on `Pause` alone is the
mapping that would be unsafe by accident, which is what the plan's correction section already says.

**The third shape in the market is delegation, and Leptos is its clearest instance. It is not
available to FLUI.** Asked how a cross-platform Rust UI framework handles this problem, the answer
from Leptos is that it does not: on Android a Leptos app runs as wasm inside a Tauri v2 WebView, so
the WebView owns the surface and Rust owns none. `leptos_wgpu` does not exist (neither crates.io
spelling resolves, the `leptos-rs` org has no wgpu integration, and the only in-repo native-render
attempt, PR #4705, was closed unmerged on 2026-05-07 with "this diff is insane"). The surface
lifecycle is delegated wholesale: `wry`'s `RustWebView` is a plain `View` drawing through HWUI into
the activity's window surface, `WryActivity` calls `mWebView.onPause()`/`onResume()`, and wry's
runtime dependencies carry no wgpu at all (wgpu is a dev-dependency for one Linux/BSD-gated example);
Tauri's own native-wgpu request (tauri#15213) is still open. The framework gets rotation, PiP,
foldables, multi-window and window teardown for free, precisely because it gave up the adapter, the
swapchain, and any ability to composite native-rendered content into its own UI. That trade is
available only to a framework whose renderer is not Rust's, which is not FLUI's position and not what
this issue is about.

**Delegation also has a leak that would matter here.** It covers the *native* surface, not a canvas
GPU context: a wgpu surface inside a WebView is canvas-backed, its loss surfaces as
`webglcontextlost`, and wgpu's wasm backend does not recover from it (gfx-rs/wgpu#3679, open since
2023-04-13). So even the delegating shape does not escape this problem; it moves which layer owns it.

**And that stack's own Android event path is the caution this plan should keep.** Tauri's lower layer
does forward the signals: `tao`'s Android JNI glue implements `onResume`/`onPause`/`onStart`/`onStop`
and maps them to `Suspended`/`Resumed`, which Tauri re-exports as public app events. But tao
deliberately swallows the first `onResume` to match iOS, and tao#949 has been open since 2024-06-30
reporting that `Resumed` is never emitted on Android at all, with the maintainer replying that the
Android backend "is a mess and barely works". None of that contradicts winit#3786, which is merged
and a different implementation. It upgrades the caveat rather than the claim: an Android event path
with no executed coverage is exactly where signals go missing, which is the same reason this plan
tiers its own evidence by strength instead of asserting that its arms work.

## What this changes in the plan

1. Decision L's "cheaper alternative" premise is wrong as written: the runner does not own a
   `poll_events` to drive the seam from. `flui-platform` owns the event loop, so **every** route
   requires `flui-platform` to observe the event, and the real choice is only what shape the signal
   takes on the way out.
2. Decision L gains a concrete second emitter (winit 0.30's `suspended()`/`resumed()`), which is a
   better argument than the hypothetical iOS background case it currently rests on.
3. Decision B gains the `rib` quote (surface-only recreation, from the dependency's maintainer) and
   bevy's before-and-after shape (window kept, surface rebuilt), and its Flutter citation is
   corrected to `onSurfaceDestroying` preceding the engine's `cleanup()`.
4. A8 gains a citation: `SurfaceTargetUnavailable` for a suspended target is already this crate's
   documented contract at `renderer.rs:1047-1051`.
5. The survey records that `.flutter` is absent and that the framework reference is silent on this
   contract by scope, so no reader mistakes the gap for an unverified parity claim.
6. Decision B gains the **third shape** the market shows (delegate the surface to the platform), its
   boundary (available only to a framework whose renderer is not Rust's), and the leak that keeps it
   from being an escape even there (`webglcontextlost`, gfx-rs/wgpu#3679).
7. Decision L's winit second-emitter claim gains its caution rather than losing it: tao#949 and the
   deliberately swallowed first `onResume` show that an unexecuted Android event path is where
   signals go missing, which is an argument for this plan's evidence tiering, not against the merge
   status of winit#3786.
