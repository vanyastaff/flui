# ADR-0152: The capability seam, revised from ADR-0084

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented.
- **Date:** 2026-10-06
- **Supersedes, on acceptance:** [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md)
  (Proposed) as a whole; what carries over is restated in §1.
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §1's last sentence ("a new capability
  is a method on `LifecycleContext`"), for platform capabilities only.
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0154](ADR-0154-capability-crates.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)

## Context

A platform capability reaches a widget through named fields today: `RuntimeHostServices` →
`RuntimeServices` → the presentation's capabilities → `BuildOwner` → `BuildCapabilities` →
`LifecycleContext` → the widget, with the producer in `flui-app`. Adding one edits flui-runtime,
flui-view and flui-app. A package built on `flui-sdk` cannot add one: `LifecycleContext` is
sealed and ADR-0078 closes its method set.

ADR-0084 proposed an open seam and has no code. Since then: `LifecycleContext` has 16 methods
(15 public), and `storage`, `close_guard` and the hidden `flush_registry_in_crate` were never
classified; owner-side handles are owner-local; system settings became one pushed snapshot
(ADR-0151 §4), which is not a pull handle; and the 1.0 direction puts optional capabilities in
their own crates (ADR-0154), which needs a provider that works without a window and handles that
end subscriptions.

The one existing platform handle, `flui_interaction::ClipboardHandle`, is owner-local
(`PhantomData<Rc<()>>`) and reads through a callback so it can become asynchronous (ADR-0038
§6); web and Wayland clipboards only read asynchronously. The seam must hand out that handle,
not the `Send + Sync` backend trait object.

## Decision

### 1. What carries over from ADR-0084

- A *framework* capability (rebuild, focus, post-frame, close guard, lifecycle handle,
  text-input route, flush registry) stays a method on `LifecycleContext`. A *platform*
  capability is reached only through `cx.capability::<C>()`.
- Providers are registered in a per-UI runtime registry passed to the UI runtime's constructor; never a
  static or thread-local. The registry is an owner-thread value: UI runtimes live on the owner thread
  whether or not the owner-thread flip lands.
- Precedence: application override, then built-in, then a single plugin. Two plugins for one
  capability, or two capabilities sharing a `NAME`, fail `Application::run` with
  `AppRunError::CapabilityConflict` before any window opens, in a deterministic order.
- No `inventory`/`linkme`. The erased entry point `capability_erased(TypeId, &'static str)` is
  hidden and object-safe; the generic `capability::<C>()` is a blanket `LifecycleContextExt`
  method, so calling it on a `BuildContext` does not compile.

### 2. What changes

1. **A capability is a marker type owned by the crate that owns its handle.** The clipboard's is
   `flui_interaction::ClipboardCapability` with `Handle = ClipboardHandle`; the handle stays
   owner-local and callback-shaped.
2. **Providers receive a context, not a window.** `provide(&ProviderContext<'_>)`;
   `ProviderContext::window()` is an `Option`. A capability that needs a window answers
   `NoWindow` itself; one that does not (storage, notifications, push) works before the first
   window. `ProviderContext` grows by methods (the host bridge of a later ADR), never by public
   fields.
3. **No cache.** The registry calls the provider on each acquisition; a provider may share state
   internally. A handle that holds an OS subscription ends it when the last clone drops, so a
   widget's `dispose` ends it.
4. **`Unsupported` is constructed, not written as a literal.** It is `#[non_exhaustive]` with
   private fields, `Unsupported::of::<C>(reason)`, and accessors; `UnsupportedReason`
   (`NotRegistered`, `NoWindow`, `NotOnThisPlatform`) is `#[non_exhaustive]` with a written
   `Display`. The registry answers `NotRegistered`; the provider answers the other two.
5. **Permissions belong to the handle.** A handle method that needs an OS permission reports a
   `Denial` (`ByUser`, `Permanently`, `Restricted`, `#[non_exhaustive]`); `PermissionState` is
   `Granted`, `NotDetermined` or `Denied(Denial)`, so an error cannot carry `Granted`. A request
   returns a boxed future so handle traits stay dyn-compatible. Whether an OS service is switched
   on is a separate state. Android and iOS usually terminate a process whose permission is
   revoked, so revocation events are best-effort. These types land with the first capability
   that needs them, not before.
6. **Classification of the later methods.** `storage` becomes a built-in capability once
   persistence lands, and `LifecycleContext::storage` is removed. `close_guard`,
   `lifecycle_handle` and `flush_registry_in_crate` are framework capabilities and stay methods.
   `clipboard_handle` is removed when the clipboard moves to the seam.
7. **Not inherited from ADR-0084:** its §5 obligation for out-of-tree backends to implement
   `PlatformAccessibility` (the core host's list is closed by ADR-0151 §3), and its haptics
   plan (haptics leaves the Stable crate and returns as a capability crate under ADR-0154).

### 3. Shape

Built and run in a scratch workspace on rustc 1.99.0 (the transcript is in the pull request).

```rust
pub trait Capability: 'static {
    type Handle: Clone + 'static;
    const NAME: &'static str;
}

#[non_exhaustive]
pub struct ProviderContext<'a> { /* window: Option<&'a Arc<dyn PlatformWindow>> */ }
impl<'a> ProviderContext<'a> {
    pub fn window(&self) -> Option<&'a Arc<dyn PlatformWindow>>;
}

pub trait CapabilityProvider<C: Capability>: 'static {
    /// # Errors
    /// [`Unsupported`] when this platform or context cannot provide `C`.
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported>;
}
const _: fn(&dyn CapabilityProvider<Probe>) = |_| {}; // dyn-compatibility pin

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{capability} is unavailable: {reason}")]
#[non_exhaustive]
pub struct Unsupported { capability: &'static str, reason: UnsupportedReason }
impl Unsupported {
    pub const fn of<C: Capability>(reason: UnsupportedReason) -> Self;
    pub const fn capability(&self) -> &'static str;
    pub const fn reason(&self) -> UnsupportedReason;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedReason { NotRegistered, NoWindow, NotOnThisPlatform }
// impl Display for UnsupportedReason — written by hand

// flui-interaction
pub enum ClipboardCapability {}
impl Capability for ClipboardCapability {
    type Handle = ClipboardHandle; // !Send
    const NAME: &'static str = "clipboard";
}
```

Checked failures: `cx.capability::<ClipboardCapability>()` on `&dyn BuildContext` is E0599;
`ClipboardHandle: Send` is E0277; a struct literal of `Unsupported` outside its crate is E0639.

`CapabilityRegistry`, `CapabilityRegistrar` and `Plugin` live in `flui-runtime` and are
re-exported by `flui-sdk`. `flui-testing` provides a headless provider for every built-in
capability.

### 4. Landing rule and first consumer

The seam merges together with its first consumer, the clipboard; no part of it reaches `main`
unwired. The landing contains the contract types, the registry, the built-in provider in
`flui-app`, the headless provider in `flui-testing`, `EditableText` acquiring the clipboard
through the seam, and the removal of `clipboard_handle`. Tests:

- copy and paste through the headless provider;
- a UI runtime with no provider answers `NotRegistered`, not a panic;
- a provider answering `NotOnThisPlatform` reaches the widget unchanged;
- two plugins for the clipboard, and two capabilities with one `NAME`, fail before a window
  opens;
- trybuild: `capability` on a `BuildContext` (E0599) and `ClipboardHandle: Send` (E0277);
- a package fixture that depends only on `flui-sdk` and the contract declares, registers and
  acquires its own capability.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Accept ADR-0084 unchanged, amend later | Its provider needs a window, it caches handles (a cached location handle keeps GPS on), it hands out the backend trait object, and it leaves three methods unclassified |
| `impl Capability for dyn Clipboard` with `Handle = Arc<dyn Clipboard>` | Loses the owner-local, asynchronous-ready `ClipboardHandle`; breaks on web and Wayland |
| One `LifecycleContext` method per capability | Packages can never add one |
| Capabilities as inherited views | Readable in `build`, which ADR-0078 forbids for capabilities |
| Tauri-style permission files in the seam | They gate a webview's IPC at build time; FLUI has no untrusted code inside the app, and OS permission is per capability and per run |

## Consequences

- One generic entry point replaces a growing method list; packages and capability crates get
  the same seam as the framework.
- `clipboard_handle` and, later, `storage` are removed in the pull requests that move them.
- The landing touches files the owner-thread and persistence work are changing; the transition
  plan gives the window and the path if the owner-thread work does not land in 0.2.0.
