# Intent: Renderer surface ownership (issue #1043)

- **Status:** Confirmed (standing autonomy; see User amendments)
- **Slug:** `1043-renderer-surface-ownership`   ·   **Date:** `2026-09-14`   ·   **Stated by:** issue #1043 (repo maintainer), session instruction by `vanyastaff`

## Asked for

Session instruction (2026-09-14, verbatim):

> надо взять issue из git и начать его решать через rust-studio скиллы и агенты и вести себя как Senior разработчик mainteiner при работе с кодом и так же при работе с github

The issue picked — the highest-severity open item (`bug`, `priority: critical`) — states the problem in the maintainer's words (issue #1043, verbatim excerpts):

> A safe public API can violate the lifetime precondition of native surface creation. This is a contract proof, not a claim that a new native crash was executed.

> `Renderer::new` accepts `&W` and returns a lifetime-free `Renderer`. It copies raw handles, creates a `Surface<'static>` through `create_surface_unsafe`, and retains no window/display owner or borrow. Its SAFETY argument assumes flui-app's App owns the window for longer than the renderer. That assumption is not a precondition a safe public caller must uphold. `#[doc(hidden)]` does not restrict access. `recover` also reuses the saved handles without reacquiring a live capability.

> A safe renderer constructor must retain a valid native surface capability until every surface operation and surface destruction is finished. Recovery must obtain handles from that live capability, not from unowned saved bytes. Safe callers must either be prevented from destroying the native target while a renderer uses it or have destruction coordinated through an explicit lease/lifecycle owner.

> Merely adding Arc is insufficient: ADR-0045 already notes that PlatformWindow::close may destroy a native window while Rust Arc owners remain. Preserve owner-thread rules and quarantine guarantees.

## What's wrong today

A crate with `#![forbid(unsafe_code)]` that depends only on `flui-engine`'s public API can write
`let r = Renderer::new(&window).await?; drop(window); r.recover().await?;` — it type-checks
(the issue's proof example, `cargo check` exit 0), and `recover()` then builds a wgpu surface
from raw handle bytes whose pointee no longer exists. Nothing in the type system, the
signature, or the docs stops it; the SAFETY argument that makes today's `unsafe` block sound
lives in a *different crate's* ownership convention (`flui-app`'s `App`).

The same saved-bytes recovery is already wrong on a shipped path, not only in the escape:
`AndroidWindow::window_handle()` returns `HandleError::Unavailable` between `Pause` and
`Resume` and yields a *different* `ANativeWindow` after resume, so a device-recovery
`recover()` that reuses the pointer captured at construction rebuilds the surface against the
old window. And `WindowsWindow::window_handle()`'s own SAFETY comment records that the handle
it hands out stays "valid" for `&self` while `DestroyWindow` may already have run — a gap the
comment names and does not close.

## What "fixed" looks like

The issue's escape does not compile (or, if a target is retained, the retained owner
demonstrably keeps the native target alive for the surface's whole life); `recover()` cannot
reach a destroyed/suspended native handle — it obtains handles from the live owner and fails
with a typed error when the owner reports them unavailable; the `RawHandles` newtype and its
manual `unsafe impl Send` are gone; the SAFETY story in `renderer.rs`, `ARCHITECTURE.md`, and
ADR-0045 describes what the code enforces, not what a consumer promises.

## Who feels it

- Embedders (the "experimental host-driven AppRuntime" audience of #560) and anyone writing a
  `flui-engine`-only integration: the constructor is `pub` and its precondition is invisible.
- The next maintainer auditing `unsafe` in `flui-engine`: today's audit has to trust a
  cross-crate convention.
- Android users on device loss after a pause/resume cycle: the stale-pointer recovery is on
  the production path (`runner/device_recovery.rs`).

## Constraints the user owns

- Standing repo rule (AGENTS.md Prime Directive #2): breaking public API is cheap now and must
  not be deferred to a shim; no consumers outside the workspace.
- Preserve the #713 (Wayland post-quit surface teardown) and #919 (programmatic close) teardown
  ordering and the ADR-0045 §7 quarantine behaviour.
- Owner-thread rules (ADR-0045 decision 1: AppKit surface work is main-thread-only; Win32
  `DestroyWindow` is owner-thread-only) must survive unchanged.

## Not this

- Not the ADR-0045 raster-lane migration (#559) or the per-owner-thread `GpuServices` adoption
  of `Renderer::new`'s eight call sites — that supersession note stays; this issue fixes the
  soundness of the constructor that exists, whichever constructor those sites move to later.
- Not a fix for AppKit's off-main-thread `recover()` panic (a known, reported ADR-0045 gap).
- Not executing native handle destruction under Miri (native GPU FFI is not interpretable);
  the ownership protocol is tested without GPU FFI, and native lifecycle coverage is added to
  the harnesses that already drive real windows.

---

## User amendments

- **2026-09-14, session start (the /loop instruction):** standing directives apply — full
  autonomy as senior architect ([[operate-as-senior-architect-full-autonomy]]), run every step
  through rust-studio ([[use-rust-studio-plugin-throughout]]), merge when green. No further
  user message about this issue has been received; the issue body is the problem statement.
- **2026-09-14, second message (during the critic pass), verbatim:**
  > поищи так же похожие проблемы в flutter issues

  Read as: survey Flutter's own issue tracker for the same defect class (surface vs. native
  window lifetime, recovery after suspend/close) so the spec's edge cases and oracles are
  informed by the reference's production history — a rule #1 cross-check, not a scope change.
- **2026-09-14, third message (same pass), verbatim:**
  > или в библиоткеках которые используют wgpu и прочитай там тоже комменты и тд

  Read as: extend the survey to the wgpu-consuming Rust frameworks (eframe/egui-wgpu, iced,
  bevy, xilem/vello, wgpu's own examples, winit's contract) — read their surface/window
  ownership code AND its comments/issues, and cite what shape each ships. Prime Directive
  rule #2 (search the market before settling); the spec's approach must cite where it comes from.

## Corrections

| Date | What changed | Why it surfaced only now |
|------|--------------|--------------------------|
| — | — | — |
