# ADR-0152: The capability seam, revised from ADR-0084

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented.
- **Date:** 2026-10-06
- **Supersedes, on acceptance:** [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md)
  (Proposed) as a whole; its design carries over except where §2 below changes it.
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §1's last sentence ("a new capability
  is a method on `LifecycleContext`") for platform capabilities only.
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) (crate names and
  `SystemPreferences`), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)

## Context

A platform capability reaches a widget through seven named fields today:
`RealmHostServices` → `RealmServices` → the presentation capabilities → `BuildOwner` →
`BuildCapabilities` → `LifecycleContext` → the widget. Adding one edits flui-runtime, flui-view
and flui-app. A package built on `flui-sdk` cannot add one at all: `LifecycleContext` is sealed
and ADR-0078 closes its method set. ADR-0084 proposed the open seam and has no code. Since it
was written:

- `LifecycleContext` has 16 methods, not 11; `storage`, `close_guard` and `lifecycle_handle`
  were added without being classified.
- The owner-thread work (send-flip) makes owner-side handles `!Send` and the realm's tables
  `Rc`-backed.
- ADR-0151 moves push-style system settings into one `SystemPreferences` snapshot delivered as
  inherited data; ADR-0084 did not separate pull handles from pushed values.
- Its code references have moved (`RealmServices` is in `flui-runtime/src/realm_services.rs`,
  `Platform::clipboard` in the backend's `traits/platform.rs`).

Other frameworks agree on three points this seam must keep: absence is a value, "not on this
platform", "not registered" and "the user refused" are distinguishable, and a fake backend is
first-class.

## Decision

### 1. What carries over from ADR-0084

A *framework* capability (rebuild, focus, post-frame, close guard, lifecycle handle, text input
route) stays a method on `LifecycleContext`. A *platform* capability is reached only through
`cx.capability::<C>()`. Providers are registered in a per-realm registry passed to the realm's
constructor, never in a static or thread-local. Precedence is app override, then built-in, then
a single plugin; two plugins for one capability fail `Application::run` with
`AppRunError::CapabilityConflict` before any window opens, in a deterministic order. No
`inventory`/`linkme`. The erased entry point `capability_erased(TypeId, &'static str)` is
hidden and object-safe; the generic `capability::<C>()` is a blanket `LifecycleContextExt`
method, so calling it in `build` does not compile. Reasons are checked in the order
`NotRegistered`, `NoWindow`, `NotOnThisPlatform`.

### 2. What changes

1. **Names.** In the contract crate `flui_platform` (ADR-0151): `Capability`,
   `CapabilityProvider`, `Unsupported`, `UnsupportedReason`. No `Platform` prefix.
2. **Owner-local handles.** `Capability::Handle: Clone + 'static`, with no `Send` bound; the
   registry is `Rc`-backed. A capability that is itself cross-thread (the clipboard) uses an
   `Arc<dyn _>` handle.
3. **Pull handles only.** System settings are not capabilities; they are `SystemPreferences`,
   published as inherited data (ADR-0151 §4).
4. **Classification of the later methods.** `storage` becomes a built-in capability once
   persistence lands, and `LifecycleContext::storage` is removed; `close_guard` and
   `lifecycle_handle` are framework capabilities and stay methods. `clipboard_handle` is
   removed when the clipboard moves to the seam.
5. **Permissions belong to the handle, not the seam.** The seam answers whether a capability
   exists here. A handle method that needs an OS permission returns an error carrying a
   `#[non_exhaustive]` `PermissionState` (`Granted`, `Denied`, `DeniedPermanently`,
   `Restricted`, `NotDetermined`), and the handle offers an asynchronous request. Revocation
   while running is reported on the handle where the OS reports it; Android and iOS usually
   terminate the process instead, so revocation is best-effort, not a guarantee. The type is
   added with the first capability that needs it.
6. **Haptics leaves `PlatformWindow`.** It is the first optional capability to become a package.
   The unreached forwarders in flui-runtime are deleted.

### 3. Shape

```rust
pub trait Capability: 'static {
    type Handle: Clone + 'static;
    const NAME: &'static str;
}

pub trait CapabilityProvider<C: Capability + ?Sized>: 'static {
    fn provide(&self, window: &Arc<dyn PlatformWindow>) -> Result<C::Handle, Unsupported>;
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{capability} is unsupported: {reason}")]
pub struct Unsupported {
    pub capability: &'static str,
    pub reason: UnsupportedReason,
}

#[non_exhaustive]
pub enum UnsupportedReason { NotRegistered, NoWindow, NotOnThisPlatform }

impl Capability for dyn Clipboard {
    type Handle = Arc<dyn Clipboard>;
    const NAME: &'static str = "clipboard";
}
```

`CapabilityRegistry`, `CapabilityRegistrar` and `Plugin` live in `flui-runtime` and are
re-exported by `flui-sdk`. `flui-testing` provides a headless provider for every built-in
capability.

### 4. First vertical slice

The clipboard: the contract types, the registry, the built-in provider in `flui-app`, the
headless provider in `flui-testing`, `EditableText` acquiring it through the seam, and
`clipboard_handle` removed. Its tests:

- a copy/paste round trip through the headless provider;
- a realm with no provider returns `Unsupported { reason: NotRegistered }`, not a panic;
- a provider that returns `NotOnThisPlatform` reaches the widget unchanged;
- two plugins registering the clipboard fail before a window opens;
- `cx.capability::<dyn Clipboard>()` in `build` fails to compile (trybuild);
- `CapabilityProvider<dyn Clipboard>` is usable as `dyn` (compile-time pin);
- a package fixture depending only on `flui-sdk` and `flui-platform` declares and registers a
  capability.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Accept ADR-0084 unchanged, amend later | It names types with a stuttering prefix, leaves three methods unclassified and predates owner-local handles; amending it right after acceptance would leave two partial sources of truth |
| Keep one `LifecycleContext` method per capability | Packages can never add a capability; each addition touches seven structures |
| Deliver capabilities as inherited views | Readable in `build`, which ADR-0078 forbids for capabilities |
| A permission model in the seam (Tauri-style capability files) | Tauri's files gate a webview's IPC at build time; FLUI has no untrusted code boundary inside the app, and OS permission is per capability and per run |

## Consequences

- One generic entry point replaces a growing method list; packages get the same seam as the
  framework.
- Two transitional entry points exist until the clipboard and storage move (the old method and
  the seam); each removal is its own pull request.
- The seam's wiring touches files the owner-thread and persistence work are changing, so the
  wiring waits for those to merge (see the platform-layer transition plan); the new modules do
  not.
