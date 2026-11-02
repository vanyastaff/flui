# ADR-0152: The capability seam, revised from ADR-0084

- **Status:** Accepted (2026-10-20). Implemented with the clipboard, its first consumer (see
  "Landing rule and first consumer" and "As built").
- **Date:** 2026-10-06
- **Revised:** 2026-10-20 — after reading how ten frameworks ship platform services (sources
  below): one result type, the first two `ProviderContext` methods, the full reason and
  permission vocabulary, `Plugin` and the registrar in the contract crate, and the carve-out for
  handles that framework widgets name. Providers of the seam are assembled by `flui-app`, not by
  the core host.
- **Amended:** 2026-10-27 — decision 2, item 7, the owner's decision: the reasons gain
  `NotYetOnThisHost` (the OS has it, this FLUI host does not implement it yet; not retryable), so
  a caller tells "never on this platform" from "not in FLUI yet". A gate that refused is still
  `NotOnThisDevice`: below the version that has the feature the device lacks it, and only from
  that version on is the host's gap `NotYetOnThisHost`. How a window carries it follows from
  this decision (ADR-0254 §6).
- **Amended:** 2026-10-31 — §5: a clipboard or primary-selection read answers
  `Result<Option<Arc<str>>, ClipboardError>`, and the readers that ask while one read is in flight
  share its answer, so a failed host read is no empty clipboard and a burst of pastes holds one
  copy of the text. The read in flight is per UI runtime, kept in
  `ProviderContext::runtime_local`, the one place for a provider's per-runtime state.
- **Amended:** 2026-10-31 — §6: a write is a `ClipboardWrite`, which a password manager marks
  `concealed` to keep it out of the clipboard's history and cloud sync, and every backend answers
  what it does with one as a `Concealment`, so an application can tell a system that honours the
  mark from one where only some clipboard managers read it and one with no way to mark it.
- **Supersedes:** [ADR-0084](ADR-0084-open-capability-seam-and-plugins.md) (Proposed) as a whole;
  what carries over is restated in §1.
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) §1's last sentence ("A new capability is a
  method on `LifecycleContext`"), for platform capabilities only.
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0153](ADR-0153-release-train-and-capability-crates.md),
  [ADR-0154](ADR-0154-capability-crates.md),
  [ADR-0254](ADR-0254-window-contract.md) (the window contract shares `Unsupported`),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md)
- **Sources:** other projects are read at pinned commits and cited as `owner/repo@sha path:line`:
  Tauri v2 (`tauri-apps/tauri@9318ab84b8`) and its plugins (`tauri-apps/plugins-workspace@11211c30e7`),
  Flutter (`flutter/flutter@6597889f57`) and its packages (`flutter/packages@1383427519`,
  `Baseflow/flutter-permission-handler@0684469188`), Capacitor (`ionic-team/capacitor@841017a3c7`),
  Expo (`expo/expo@0937842df7`), Compose Multiplatform
  (`JetBrains/compose-multiplatform-core@13b7599795`), Bevy (`bevyengine/bevy@2614884437`), GPUI
  (`zed-industries/zed@fae8a35482`), Project Robius (`project-robius/robius-location@1974355613`),
  `rust-mobile/ndk-context@0eb252bf23`, Crux (`redbadger/crux@1ef0f547e6`) and the XDG portals
  (`flatpak/xdg-desktop-portal@49075617ff`).

Crate names follow ADR-0151: the contract crate is `flui-platform` and the core host is
`flui-native`.

## Context and problem statement

A platform capability reaches a widget through named fields today: `RuntimeHostServices` →
`RuntimeServices` → the presentation's capabilities → `BuildOwner` → `BuildCapabilities` →
`LifecycleContext` → the widget, with the producer in `flui-app`. Adding one edits flui-runtime,
flui-view and flui-app. A package built on `flui-sdk` cannot add one: `LifecycleContext` is
sealed and ADR-0078 closes its method set.

ADR-0084 proposed an open seam and has no code. Since then: `LifecycleContext` has 16 methods
(15 public), and `storage`, `close_guard` and the hidden `flush_registry_in_crate` were never
classified; owner-side handles are owner-local; system settings became one pushed snapshot
(ADR-0172), which is not a pull handle; and the 1.0 direction puts optional capabilities in their
own crates (ADR-0154), which needs a provider that works without a window and handles that end
their subscriptions.

The one existing platform handle, `flui_interaction::ClipboardHandle`, is owner-local
(`PhantomData<Rc<()>>`) and reads through a callback so it can become asynchronous (ADR-0038
§6); web and Wayland clipboards only read asynchronously. The seam must hand out that handle, not
a `Send + Sync` provider trait object. The synchronous `Clipboard` trait cannot be implemented
honestly on those two platforms, and keeping it beside an asynchronous twin (as GPUI does) is two
shapes for one fact.

Reading the references showed what the first design did not yet say:

- Every Rust framework with a typed seam keys it by type and scopes it to the application (GPUI
  globals, [zed-industries/zed@fae8a35482 crates/gpui/src/global.rs:22](https://github.com/zed-industries/zed/blob/fae8a35482/crates/gpui/src/global.rs#L22); Bevy resources; Tauri
  managed state, [tauri-apps/tauri@9318ab84b8 crates/tauri/src/lib.rs:752-771](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/lib.rs#L752-L771)). The defects sit in
  the designs with a process singleton (Flutter's static `_instance`,
  [flutter/packages@1383427519 packages/url_launcher/url_launcher_platform_interface/lib/src/url_launcher_platform.dart:27-41](https://github.com/flutter/packages/blob/1383427519/packages/url_launcher/url_launcher_platform_interface/lib/src/url_launcher_platform.dart#L27-L41);
  Capacitor's registry; `ndk-context`'s `static mut ANDROID_CONTEXT`,
  [rust-mobile/ndk-context@0eb252bf23 ndk-context/src/lib.rs:28](https://github.com/rust-mobile/ndk-context/blob/0eb252bf23/ndk-context/src/lib.rs#L28)).
- Tauri ships a blocking and an asynchronous twin of each mobile call
  (`run_mobile_plugin` and `run_mobile_plugin_async`, [tauri-apps/tauri@9318ab84b8
  crates/tauri/src/plugin/mobile.rs:293-360](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/plugin/mobile.rs#L293-L360)) and fixed deadlocks in the response path, where a
  lock was held while the response was delivered ([mobile.rs:98-100](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/plugin/mobile.rs#L98-L100), [:121-123](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/plugin/mobile.rs#L121-L123), [:420-422](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/plugin/mobile.rs#L420-L422));
  FLUI's drafts had a callback clipboard and a boxed-future permission request, so three shapes for
  "the OS answers later".
- OS callbacks arrive off the owner thread on every platform the first crates target, and an
  owner-local handle gives the provider no way back to the owner. Project Robius requires its
  Apple manager to be created on the main thread and takes a `Send + Sync` handler
  ([project-robius/robius-location@1974355613 src/lib.rs:28,76](https://github.com/project-robius/robius-location/blob/1974355613/src/lib.rs#L28)).
- Real permissions have scopes (precise or coarse, when-in-use or always) and reduced grants
  (limited, provisional, approximate); a three-way state cannot say what was granted. Tauri's
  geolocation status is `{ location, coarse_location }`
  ([tauri-apps/plugins-workspace@11211c30e7 plugins/geolocation/src/models.rs:12-27](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/src/models.rs#L12-L27)), the
  permission handler for Flutter splits `locationWhenInUse` from `locationAlways`
  ([Baseflow/flutter-permission-handler@0684469188
  permission_handler_platform_interface/lib/src/permissions.dart:64-78](https://github.com/Baseflow/flutter-permission-handler/blob/0684469188/permission_handler_platform_interface/lib/src/permissions.dart#L64-L78)) and its statuses include
  `limited` and `provisional` ([.../permission_status.dart:4-60](https://github.com/Baseflow/flutter-permission-handler/blob/0684469188/permission_handler_platform_interface/lib/src/permission_status.dart#L4-L60)).
- Tauri's geolocation plugin shipped the bug that acquisition in lifecycle hooks and a handle that
  ends its subscription on drop rule out: its 2.5.0 release fixed `watchPosition` stopping updates
  for every other active watch and `clearWatch` leaving location updates running until the last
  watch was cleared ([tauri-apps/plugins-workspace@11211c30e7 plugins/geolocation/CHANGELOG.md](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/CHANGELOG.md)).

The question this record answers: how does a platform capability reach a widget so that the
framework, a capability crate and a package built on `flui-sdk` add one through the same seam,
without editing `flui-runtime`, `flui-view` and `flui-app` and without process-global state?

## Decision drivers

- The frame path is synchronous: no `async` in build, layout or paint, so "the OS answers later" is
  a value consumed by callback or poll, with a `Future` adapter only outside the frame path.
- A package built on `flui-sdk` must be able to add a capability, and `LifecycleContext` is sealed
  with a closed method set (ADR-0078).
- No process-global state: providers live per UI runtime, and a native-to-Rust trampoline carries
  its state in the native object (ADR-0097).
- OS callbacks arrive off the owner thread on every platform the first crates target, and an
  owner-local handle gives the provider no way back to the owner.
- The one existing handle, `ClipboardHandle`, is owner-local (`!Send`) and reads through a callback,
  because web and Wayland clipboards only read asynchronously.
- "The OS answers later" has one shape; re-entry with guards or borrows held is the first of the
  recurring defects (AGENTS.md).
- A handle that holds an OS subscription needs an owner that ends it: acquisition in lifecycle
  hooks, no cache, and the subscription ends when the last clone drops.
- Real permissions have scopes and reduced grants, which a three-way state cannot express.
- Substituting an implementation must not mean replacing the plugin.
- A capability crate depends on one small surface, so the seam names `WindowRef` and not
  `PlatformWindow`, which keeps the semver surface narrow (ADR-0153).
- Rules are types: `capability` on a `BuildContext` must not compile, and an owner-local handle
  must not be `Send`.
- The seam merges together with its first consumer; no part of it reaches `main` unwired.

## Considered options

1. The revised seam decided below: capability markers keyed by type, providers per UI runtime,
   handles acquired in lifecycle hooks, `Completion<T>`, `Unsupported` and `Plugin` in the contract
   crate.
2. Accept ADR-0084 unchanged, amend later.
3. A synchronous `Clipboard` trait object as the handle, with an asynchronous twin (GPUI).
4. A process singleton per service (Flutter's static instance, Capacitor's registry).
5. The handle type as the key (Tauri's `state::<T>()`).
6. Twin blocking and asynchronous APIs (Tauri).
7. `async fn` in handle traits as the base.
8. Callbacks only.
9. Effects as data (Crux; [redbadger/crux@1ef0f547e6 crux_core/src/testing.rs:8-12](https://github.com/redbadger/crux/blob/1ef0f547e6/crux_core/src/testing.rs#L8-L12)).
10. The whole `PlatformWindow` in `ProviderContext`.
11. One `LifecycleContext` method per capability.
12. Capabilities as inherited views, readable anywhere (Compose, GPUI).
13. Tauri-style permission files in the seam.

## Decision

Chosen option: 1, because it keeps what ADR-0084 got right (an open seam, a per-UI-runtime
registry, no `inventory`) and fixes what reading ten frameworks showed it lacked: one owner-thread
result type for "the OS answers later", providers that need no window, handles that end their
subscriptions on drop, a `Plugin` that leaf crates can reach, and absence and permissions as typed
values.

### 1. What carries over from ADR-0084

- A *framework* capability (rebuild, focus, post-frame, close guard, lifecycle handle,
  text-input route, flush registry) stays a method on `LifecycleContext`. A *platform*
  capability is reached only through `cx.capability::<C>()`.
- Providers are registered in a per-UI runtime registry passed to the UI runtime's constructor;
  never a static or thread-local. The registry is an owner-thread value: UI runtimes live on the
  owner thread whether or not the owner-thread flip lands.
- Precedence: application override, then built-in, then a single plugin. Two plugins for one
  capability, or two capabilities sharing a `NAME`, fail `Application::run` with
  `AppRunError::CapabilityConflict` before any window opens, in a deterministic order.
- No `inventory`/`linkme`. The erased entry point `capability_erased(TypeId, &'static str)` is
  hidden and object-safe; the generic `capability::<C>()` is a blanket `LifecycleContextExt`
  method, so calling it on a `BuildContext` does not compile.

ADR-0084 also rested on a premise that no longer holds: that Flutter's federated plugins give an
application no way to replace an endorsed implementation
([flutter/flutter#80374](https://github.com/flutter/flutter/issues/80374)). The issue closed on
2024-07-11, and the Flutter tool now resolves a direct dependency over the default package
([flutter/flutter@6597889f57 packages/flutter_tools/lib/src/flutter_plugins.dart:1950-1993](https://github.com/flutter/flutter/blob/6597889f57/packages/flutter_tools/lib/src/flutter_plugins.dart#L1950-L1993)). Nothing
in this ADR depends on that claim. Substitution in FLUI is a provider override because the registry
is keyed by the marker, not by the implementation.

### 2. What the seam is

1. **A capability is a marker type owned by the crate that owns its handle.** The clipboard's is
   `flui_interaction::ClipboardCapability` with `Handle = ClipboardHandle`; the handle stays
   owner-local and `!Send`. The same rule decides where a capability's marker lives when framework
   widgets name its handle (item 10).
2. **Providers live per UI runtime, and a handle is acquired only in a lifecycle hook.**
   `cx.capability::<C>()` exists on `LifecycleContext`, whose hooks are `init_state` and
   `did_change_dependencies`; on a `BuildContext` it does not compile (ADR-0078). A handle that
   holds an OS subscription then has an owner, the widget's state.
3. **No cache.** The registry calls the provider on each acquisition; a provider may share state
   internally. A handle that holds an OS subscription ends it when the last clone drops, so a
   widget's `dispose` ends it. Work that must outlive the widget that started it (background
   location, push delivery) is not a widget-held handle: it is a service of the application
   (ADR-0049).
4. **`Plugin` and `CapabilityRegistrar` live in the contract crate; the registry stays in
   `flui-runtime`.** The registrar is a list of boxed providers and needs nothing from the runtime.
   A leaf capability crate (ADR-0154) registers its providers and therefore needs `Plugin`; with
   `Plugin` in the runtime every capability crate would depend on `flui-sdk` or `flui-runtime` just
   to register. `CapabilityRegistry`, conflict detection and the run loop stay in `flui-runtime`;
   `flui-sdk` re-exports `Plugin` and the registrar for packages.
5. **One result type for "the OS answers later": `Completion<T>`.** It is owner-thread (`!Send`)
   and resolves at an owner turn boundary: never inline in the call that created it, never from
   an OS callback, and never with a guard or borrow held. It is consumed by callback or by poll,
   and has a `Future` adapter for code outside the frame path (services, tasks); the frame path
   stays synchronous. An OS callback only posts to the owner; everything else runs there. Handle
   methods that finish later return it: `ClipboardHandle`'s reads move from a bare callback to it,
   and a permission request returns `Completion<PermissionState>` rather than a boxed future.
   Handle traits stay dyn-compatible because `Completion<T>` is a concrete type.

   What happens when the owner retires before the result arrives is stated, not left to chance:
   the `on_complete` closures of every pending completion are dropped without running, `poll`
   answers `None` from then on, and a post that arrives after retirement is refused by the poster
   (item 6), so the provider learns that its subscription has lost its owner and ends it. A
   completion that was resolved before retirement is delivered: a result the owner accepted is
   never dropped once accepted, and retirement is the one boundary at which an unresolved one
   ends.
6. **Providers receive a context, not a window.** `provide(&ProviderContext<'_>)`. Its first two
   methods are:
   - `owner()` returns a cloneable `Send + Sync` poster that runs a closure on the owner thread.
     It is what a provider's OS-callback trampoline uses to get back. Its `post` answers
     `Err(OwnerRetired)` once the owner has retired, so a trampoline can end the OS subscription
     it serves instead of posting into nothing; a post the owner accepted runs at its next turn.
   - `window()` returns `Option<WindowRef>`: the window's id, its scale and its raw window and
     display handles, and no other method of `PlatformWindow`. Portals need a raw handle, Win32
     and AppKit dialogs need an owner handle, and every `PlatformWindow` method a capability crate
     names would be semver surface for all of them (ADR-0153). The handles are exposed through
     `HasWindowHandle` and `HasDisplayHandle`, the one upstream exception ADR-0089 §2 allows in a
     stable signature.

   A capability that needs a window answers `NoWindow` itself; one that does not (storage,
   notifications, push) works before the first window. `ProviderContext` grows by methods (the
   host bridge of ADR-0154), never by public fields.
7. **Absence is one value, `Unsupported { reason }`.** It is constructed, not written as a
   literal: `#[non_exhaustive]` with private fields, built by `Unsupported::new(reason)` or, when
   a capability names it, by `Unsupported::of::<C>(reason)`, and read through accessors.
   It is defined once in the contract crate and shared with the window contract (ADR-0254), which
   embeds it in its errors in place of separate unit variants. `UnsupportedReason` is
   `#[non_exhaustive]` with a written `Display`. No reason names a version of the OS, so no
   application branches on one:

   | Reason | Meaning |
   |---|---|
   | `NotOnThisPlatform` | This platform never has it. |
   | `NotOnThisDevice` | This platform has it, but the OS build or the hardware of this device lacks it (ADR-0244's gate refused, or the sensor is absent). The value carries no version number; the number stays inside the gate. |
   | `NotYetOnThisHost` | This platform's OS has it, but this FLUI host does not implement it yet. Not retryable: the same host answers the same way. A later FLUI may remove it, which `NotOnThisPlatform` never invites. |
   | `NotDeclared { what }` | The application's manifest lacks something the capability needs (a permission, a usage string, a background mode); `what` names it. The declarations of ADR-0154 make this detectable before the OS refuses or ends the process. |
   | `Unavailable` | Temporary: the capability can work here but not now, and the caller may try again. Not the user's refusal (a permission state) and not an OS service switched off (service state). |
   | `NotRegistered` | The UI runtime has no provider. Answered by the registry. |
   | `NoWindow` | The capability needs a window and the context has none. Answered by the provider. |

8. **Permissions belong to the handle.** A handle method that needs an OS permission reports
   `PermissionState`:
   - `Granted(Grant)`, `NotDetermined` or `Denied(Denial)`, so an error cannot carry `Granted`.
     `Grant` is `Full`, `Reduced` (less than asked: limited photos, approximate location) or
     `Provisional` (delivered without asking, pending the user's choice). `Denial` is `ByUser`,
     `Permanently` or `Restricted`. All three are `#[non_exhaustive]`.
   - A capability that has scopes names them in a `Scope` associated type (precise or coarse,
     when-in-use or always); its handle answers `permission(scope)` and requests with
     `request(scope)`, which returns `Completion<PermissionState>`.
   - Whether an OS service is switched on is a separate state on the handle, not a permission:
     "no permission" and "location services off" are different facts to a widget.
   - A permission *check* on Android never returns `Denied(Permanently)`: the platform reveals it
     only as the result of a request. The Android provider's tests hold it.
   - Android and iOS usually end a process whose permission is revoked, so revocation events are
     best effort.

   The shape of these types is decided here. The types themselves are added with the first
   capability that needs a permission, notifications or location, and not before: `flui-system`,
   the first capability crate (ADR-0154), has no permission consumer.
9. **Providers keep no process-global state.** A native-to-Rust trampoline carries its state in
   the native object: a handle field on the Java object bound with `RegisterNatives` (ADR-0247), an
   `objc2` instance variable, a captured closure. ADR-0097's single trampoline cell stays the only
   one. This is a conformance requirement of every provider (ADR-0154).
10. **When framework or package widgets name a handle, its marker and handle stay in the lowest
    crate that names it, and only the OS provider lives elsewhere** (in `flui-app` for a service
    on the core host's closed list, in a capability crate for the rest). The clipboard
    (`flui-interaction`) is the model, and opening links (a `Link` widget) is the other case.
    Haptics is not one: it is a facet of the window (ADR-0210, ADR-0254), reached through the
    window, and does not travel through the seam. This is item 1 applied, not an exception to it:
    Flutter's `Link` and `LinkDelegate` sit in the platform-interface package
    ([flutter/packages@1383427519 packages/url_launcher/url_launcher_platform_interface/lib/src/url_launcher_platform.dart:9,44](https://github.com/flutter/packages/blob/1383427519/packages/url_launcher/url_launcher_platform_interface/lib/src/url_launcher_platform.dart#L9))
    and Compose keeps its clipboard and URI locals in `ui`
    ([JetBrains/compose-multiplatform-core@13b7599795
    compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/platform/CompositionLocals.kt:74-269](https://github.com/JetBrains/compose-multiplatform-core/blob/13b7599795/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/platform/CompositionLocals.kt#L74-L269))
    for the same reason. ADR-0154 states the matching rule for capability crates.

    The clipboard and storage are services the framework needs before any capability crate exists,
    so `flui-app`, the composition root, supplies their providers as built-ins. The providers
    cannot sit in the core host: the marker `ClipboardCapability` is in `flui-interaction`, which
    the core host cannot depend on. The core host exposes the OS objects the providers wrap, an
    owner-local clipboard that reads through a callback and `FileStore`. **The synchronous
    `Clipboard` trait is deleted** when the seam lands with the clipboard as its first consumer;
    the clipboard methods of `Platform` go with it.
11. **Classification of the later methods.** `storage` becomes a built-in capability once
    persistence lands, and `LifecycleContext::storage` is removed. `close_guard`,
    `lifecycle_handle` and `flush_registry_in_crate` are framework capabilities and stay methods.
    `clipboard_handle` is removed when the clipboard moves to the seam.
12. **Not inherited from ADR-0084:** its obligation for out-of-tree hosts to implement
    `PlatformAccessibility` (the core host's list is closed by ADR-0151), and its haptics plan
    (haptics is a facet of the window, ADR-0210 and ADR-0254; its vocabulary stays in the contract
    with its OS paths in the core host, and it does not travel through the seam).

### 3. Shape

The 2026-10-06 shape (marker, provider, the dyn-compatibility pin, `Unsupported` as a constructed
value and the three checked compile failures below) was built and run in a scratch workspace on
rustc 1.99.0. The additions of this revision have not been built yet; the first landing builds
them, and the spelling of a new name below may change in detail while the properties above hold.

```rust
// flui-platform (the contract crate)
pub trait Capability: 'static {
    type Handle: Clone + 'static;
    const NAME: &'static str;
}

#[non_exhaustive]
pub struct ProviderContext<'a> { /* private */ }
impl<'a> ProviderContext<'a> {
    /// A poster that runs a closure on the owner thread; callable from any thread.
    pub fn owner(&self) -> &OwnerPoster;
    /// The window the acquiring widget belongs to, if any.
    pub fn window(&self) -> Option<WindowRef>;
}
// OwnerPoster: Clone + Send + Sync
impl OwnerPoster {
    /// Runs `f` at the owner's next turn.
    /// # Errors
    /// [`OwnerRetired`] once the owner has retired; `f` is dropped unrun.
    pub fn post(&self, f: impl FnOnce() + Send + 'static) -> Result<(), OwnerRetired>;
}

/// Id, scale and raw handles of a window; no `PlatformWindow` method is reachable.
pub struct WindowRef { /* private */ }
impl HasWindowHandle for WindowRef { /* .. */ }
impl HasDisplayHandle for WindowRef { /* .. */ }

pub trait CapabilityProvider<C: Capability>: 'static {
    /// # Errors
    /// [`Unsupported`] when this platform or context cannot provide `C`.
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported>;
}
const _: fn(&dyn CapabilityProvider<Probe>) = |_| {}; // dyn-compatibility pin

/// The result of an operation the OS finishes later. Owner-thread only.
#[must_use]
pub struct Completion<T> { /* private; !Send */ }
impl<T: 'static> Completion<T> {
    /// Runs `f` with the result at an owner turn boundary.
    pub fn on_complete(self, f: impl FnOnce(T) + 'static);
    /// Takes the result if it has arrived.
    pub fn poll(&mut self) -> Option<T>;
    /// For code outside the frame path; polled on the owner thread.
    pub fn into_future(self) -> impl Future<Output = T>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unsupported { /* private: the capability's NAME when there is one, the reason */ }
impl Unsupported {
    pub const fn new(reason: UnsupportedReason) -> Self;
    pub const fn of<C: Capability>(reason: UnsupportedReason) -> Self; // names the capability
    pub const fn reason(&self) -> UnsupportedReason;
}
// impl Display and Error — written by hand; the message names the capability when there is one

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedReason {
    NotOnThisPlatform,
    NotOnThisDevice,
    NotYetOnThisHost,
    NotDeclared { what: &'static str },
    Unavailable,
    NotRegistered,
    NoWindow,
}
// impl Display for UnsupportedReason — written by hand

// The permission types below are the decided shape. They enter the contract with the first
// capability that needs a permission (notifications or location), not with the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PermissionState { Granted(Grant), NotDetermined, Denied(Denial) }
#[non_exhaustive] pub enum Grant { Full, Reduced, Provisional }
#[non_exhaustive] pub enum Denial { ByUser, Permanently, Restricted }

// flui-interaction
pub enum ClipboardCapability {}
impl Capability for ClipboardCapability {
    type Handle = ClipboardHandle; // !Send
    const NAME: &'static str = "clipboard";
}
```

How a provider resolves a `Completion` from an OS callback is internal to the contract crate: the
callback's only act is to post through the owner poster.

Checked failures: `cx.capability::<ClipboardCapability>()` on `&dyn BuildContext` is E0599;
`ClipboardHandle: Send` is E0277; a struct literal of `Unsupported` outside its crate is E0639.
The landing adds `Completion<T>: Send` as E0277.

### 4. Landing rule and first consumer

The seam merges together with its first consumer, the clipboard; no part of it reaches `main`
unwired. Storage follows as the second built-in capability, and `flui-system` (ADR-0154) as the
first capability crate. The clipboard landing contains the contract types, the registry in
`flui-runtime`, the built-in clipboard provider in `flui-app` (the composition root; the marker
sits above the core host) over the core host's owner-local clipboard, the headless host's
clipboard (`pub(crate)` in the core host), a scripted double in `flui-testing` built on the
contract's `scripted` feature, `EditableText` acquiring the clipboard through the seam, and the
removal of `clipboard_handle`, the synchronous `Clipboard` trait and the contract's
`InMemoryClipboard`. The permission types are not part of it. Tests:

- copy and paste through the headless clipboard;
- a UI runtime with no provider answers `NotRegistered`, not a panic;
- a provider answering `NotOnThisPlatform` reaches the widget unchanged;
- two plugins for the clipboard, and two capabilities with one `NAME`, fail before a window
  opens;
- a result the OS delivers on a thread other than the owner's reaches the widget at an owner turn
  boundary, through the poster;
- a post after the owner retired answers `OwnerRetired` and runs nothing, a completion pending at
  retirement drops its closure unrun and polls `None`, and a completion resolved before retirement
  is delivered;
- trybuild: `capability` on a `BuildContext` (E0599), `ClipboardHandle: Send` and
  `Completion<T>: Send` (E0277);
- a package fixture that depends only on `flui-sdk` and the contract declares, registers and
  acquires its own capability, and so does a leaf-crate fixture that depends on the contract alone.

### 5. Clipboard reads share one answer and name their failure

Amendment of 2026-10-31. A read answered `Option<String>`: every read held its own copy of the
text, so a burst of pastes of a large clipboard held as many copies as readers, and a host that
could not read answered `None`, which a caller could not tell from an empty clipboard. The
clipboard and the primary selection now answer one shape:

```rust
pub type ClipboardRead = Result<Option<Arc<str>>, ClipboardError>;

#[non_exhaustive]
pub enum ClipboardError { Unavailable, Denied, Timeout, Malformed, TooLarge }

impl ClipboardHandle {
    pub fn new(backend: impl ClipboardBackend, cx: &ProviderContext<'_>) -> Self;
    pub fn read_text(&self) -> Completion<ClipboardRead>;
}
pub trait ClipboardBackend { fn read_text(&self, answer: Resolver<ClipboardRead>); /* … */ }

impl ProviderContext<'_> { pub fn runtime_local<T: Default + 'static>(&self) -> Rc<T>; }
```

The primary selection's handle and backend take the same types.

- **One read in flight per UI runtime.** Every handle a UI runtime hands out shares one read in
  flight, whichever widget or window acquired it. A reader that asks while it is in flight joins
  it; the first reader starts the host read. When the host answers, every reader settles with a
  clone of the same `ClipboardRead`, so they all hold one allocation of the text, and ten fields
  pasting at once ask the host once. The next read after the answer starts afresh. The read lives
  and ends with its runtime, so a runtime retiring never strands another's readers. The X11
  thread still refuses reads past `MAX_READERS` waiting at once, as `Unavailable`, a backstop
  for many runtimes reading together.
- **Per-runtime provider state.** A provider is registered once and asked by every UI runtime,
  so state it holds itself is shared across them. `ProviderContext::runtime_local::<T>()` is the
  one place a provider keeps state for one runtime, shared by every handle it hands out there:
  a typed slot per `T`, made with `T::default()` on first ask, which a capability names with a
  private type. The handles' constructors take the context and find their slot there, so no
  provider has to remember to share it. Later capabilities keep their per-runtime state the same
  way (an HTTP pool, an open store).
- **Drop order at retirement.** The runtime ends its pending completions first, then drops the
  jobs still queued, then retires its runtime-local slots, each drop contained and the first
  failure kept. A completion's closure dropped in the first step still finds its provider's
  slot alive. A slot a handle still holds ends when that handle drops; an acquisition after
  retirement gets a fresh value kept nowhere.
- **Containment.** No borrow of the shared state is held while the backend runs. Each reader's
  delivery is contained, so a wake hook that panics leaves the readers after it answered and
  its failure goes on after them. A write, or a primary-selection claim or its release, through
  any handle of the runtime ends the sharing: a read asked after it never joins one asked
  before. A backend that drops its resolver, or panics and drops it, abandons the read: every
  reader that joined it settles `Abandoned::ProviderDropped`. A runtime that retires before the
  answer drops the waiting closure unrun; the readers go with it, and a later read never finds
  the stale one, because a read clears only its own entry.
- **Ok(None) means no text.** A clipboard that holds nothing, or only data that is not text,
  answers `Ok(None)`. `Unavailable` is a clipboard the host could not reach (another application
  holds it open, the display connection is gone, a host without one). `Denied` is a permission:
  the browser's `NotAllowedError`. `Timeout` is a selection owner that did not answer in time
  (X11, Wayland). `Malformed` is text invalid in its encoding (an unterminated `CF_UNICODETEXT`,
  `arboard`'s conversion failure). `TooLarge` is text past 64 MiB, one limit on every host,
  refused before FLUI copies it (decoded text measured first on Win32, X11 and Wayland). On
  the web, a refused read still answers what the page last wrote, and names its refusal only
  before the page wrote anything.
- **Cost.** An answer takes one more owner job than before: the host's answer settles the shared
  read, which then resolves each reader.

### 6. A write says whether it is concealed

Amendment of 2026-10-31. A write was a `String`: a password manager that copied a secret put it
in Windows' clipboard history and cloud clipboard, in macOS's and KDE's clipboard managers and on
the user's other Apple devices, with no way to ask otherwise. A write now names what it asks:

```rust
#[non_exhaustive]
pub struct ClipboardWrite { /* text, concealed */ }

impl ClipboardWrite {
    pub fn text(text: impl Into<String>) -> Self;
    pub fn concealed(self) -> Self;
    pub fn as_text(&self) -> &str;
    pub fn is_concealed(&self) -> bool;
    pub fn into_text(self) -> String;
}

#[non_exhaustive]
pub enum Concealment { Honored, HintOnly, Unavailable }

impl ClipboardHandle {
    pub fn write(&self, write: ClipboardWrite);
    pub fn write_text(&self, text: impl Into<String>); // write(ClipboardWrite::text(text))
    pub fn concealment(&self) -> Concealment;
}
pub trait ClipboardBackend {
    fn write(&self, write: ClipboardWrite);
    fn concealment(&self) -> Concealment;
    /* read_text */
}
```

`flui_native::NativeClipboard` takes the same `write` and `concealment`, with `write_text` as its
shorthand.

- **A struct, not a flag.** A builder grows (an expiry, a second form of the text) without
  another method on every backend, and `write_text` stays the short form for an ordinary copy.
  Its `Debug` shows the length and the mark, never the text.
- **What each host does.**

  | Host | A concealed write | `concealment()` |
  |------|-------------------|-----------------|
  | Win32 | `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` and `CanUploadToCloudClipboard`, each the DWORD 0, beside the text | `Honored` |
  | AppKit | `org.nspasteboard.ConcealedType` and `org.nspasteboard.TransientType`, and `prepareForNewContentsWithOptions(CurrentHostOnly)`, which keeps it off the universal clipboard | `Honored` |
  | UIKit | `setItems:options:` with `UIPasteboardOptionLocalOnly`; iOS keeps no history | `Honored` |
  | `arboard` (winit) | its `exclude_from_history`: Win32's formats, the nspasteboard.org concealed type, KDE's hint | `Honored` on Windows, `HintOnly` elsewhere |
  | X11, Wayland | the target or MIME type `x-kde-passwordManagerHint`, answered `secret` | `HintOnly` |
  | Android | `ClipDescription.EXTRA_IS_SENSITIVE` in the clip's extras (API 24 and later) | `HintOnly`, `Unavailable` below API 24 |
  | Web | an ordinary `writeText`: the Clipboard API has no mark; a debug build warns once per clipboard | `Unavailable` |
  | Headless, in memory | the text; nothing keeps a history | `Honored` |

- **Honored, HintOnly, Unavailable.** `Honored` is the system keeping the entry out of its own
  history and sync. `HintOnly` is a mark only what reads it honours: a clipboard manager on
  Linux, the copy preview and keyboards on Android. `Unavailable` is a host with no mark, where a
  concealed write is an ordinary one. The answer is a property of the backend, constant for its
  life, so an application asks before it offers to copy a secret, not after.
- **A mark that fails drops the write.** On Win32 and AppKit the markers are set before the text,
  in one clipboard session; a marker the system refuses leaves the clipboard empty rather than
  hand the secret to the history. On Android a clip that could not be marked is not set.
- **Gap 26 is a scenario.** `flui_native::testing::clipboard::a_concealed_write_stays_out_of_history`
  is the one scenario every host's clipboard runs: the concealed write pastes back whole, and the
  host's answer is not `Unavailable`. A host that answers `Unavailable` fails it as gap 26, which
  its test names with `expect_fail(CONCEALED_WRITES, …)` rather than skipping it; that call fails
  once the host conceals, so the fix closes the gap and the call together.

### As built

The clipboard landing spells the shape of §3 as follows; the properties of §2 hold as written.

- **A completion always ends.** `Completion<T>` settles with `Result<T, Abandoned>`:
  `on_complete` takes `FnOnce(Result<T, Abandoned>)`, `poll` answers
  `Option<Result<T, Abandoned>>` and the future yields the same. `Abandoned::ProviderDropped` is a
  `Resolver` dropped unanswered, `Abandoned::OwnerRetired` a UI runtime that dropped first, so no
  waiter hangs; a value settled before retirement is still delivered.
- **Where completions come from.** `ProviderContext::completions()` is the UI runtime's
  `CompletionSource`, whose `pending::<T: Send>()` returns the `(Completion<T>, Resolver<T>)`
  pair. The closures waiting on completions live in the owner-local `PendingCompletions`; a
  settle job carries only a generational slot id.
- **The owner-job queue.** The poster sends to an unbounded queue of its own, not to the bounded
  command inbox, where backpressure would lose an answer. The UI runtime drains it after the
  inbox at every Idle drain, bounded by the length it read first, and retires its completions
  when it drops.
- **Refusals name the capability.** `Unsupported::of::<C>` and its erased form
  `Unsupported::named` set the name `Unsupported::capability` returns.
- **The registry.** `flui_runtime::capability::CapabilityRegistry::builder()` takes
  `overrides`, `built_ins` and each `plugin`; `build` answers `CapabilityConflict::Duplicate`
  (one capability twice in one tier, checked in every tier even where a higher one answers) or
  `CapabilityConflict::SharedName`. An application registers through
  `flui_app::AppCapabilities` (`capability::<C>(provider)`, `plugin(plugin)`), which every
  platform's one entry point takes in its `AppLaunch` (`run_app`, `run_app_android`), and which
  `Application::capability` and `Application::plugin` build on the desktop. The conflict is found
  before the platform is created: the entry point answers `LaunchError::CapabilityConflict`,
  `Application::run` answers `AppRunError::CapabilityConflict`. `RuntimeHostServices` takes
  the registry through `with_capabilities`; without one every acquisition answers
  `NotRegistered`.
- **The core host's clipboard.** `flui_native::NativeClipboard` is owner-local and
  `OwnerPlatform::clipboard` hands it out; its read answers through a `Resolver`. Win32, AppKit,
  UIKit and `arboard` answer at once, the web when `navigator.clipboard.readText()` settles
  (falling back to what the page wrote when the browser refuses). Android reads and writes
  `ClipboardManager` through JNI.
- **The scripted double.** `flui_testing::ScriptedClipboard` holds, answers, fails, drops or
  refuses reads, and counts the reads it was asked and the live handles;
  `HeadlessHost::with_capabilities` and `HeadlessHost::with_plugin` mount a tree over another
  registry. `CompletionSource` is public, so the contract needs no
  `scripted` feature; ADR-0243 names test support `testing` in any case.
- **Checked failures.** `capability` on a `BuildContext` is the E0599 doctest of
  `LifecycleContextExt`; `ClipboardHandle: Send` and `Completion<T>: Send` are the E0277 cases
  `a_clipboard_handle_stays_on_its_thread` (`flui-interaction`) and
  `a_completion_cannot_cross_a_thread` (`flui-platform`).
- **The §4 tests** are the `capability_seam` table of `flui-testing`,
  `a_package_declares_registers_and_acquires_its_own_capability` (`flui-sdk`),
  `a_leaf_crate_declares_registers_and_acquires_on_the_contract_alone` (`flui-platform`) and
  `conflicting_registrations_fail_run_before_the_platform_starts` (`flui-app`).

### Consequences

- Good, because one generic entry point replaces a growing method list; packages and capability
  crates get the same seam as the framework, and a leaf capability crate registers with the
  contract alone.
- Neutral, because `clipboard_handle` and, later, `storage` are removed in the pull requests that
  move them, and the platform-capability row of `docs/extending.md` changes in the clipboard
  landing.
- Good, because the seam names only contract types, a `WindowRef` and not `PlatformWindow`, which
  keeps the surface capability crates depend on small; that is what makes the independent
  versioning of ADR-0153 reachable.
- Good, because every call the OS answers later has one shape, so a test can script it
  (ADR-0154), and an OS callback has one job: post.
- Bad, because a widget still cannot take a capability in `build`; long-lived OS work therefore
  belongs to an application service, not to a handle held by a widget.

### Confirmation

- The compiler, through the checked failures of §3: `cx.capability::<ClipboardCapability>()`
  on `&dyn BuildContext` is E0599, `ClipboardHandle: Send` is E0277, a struct literal of
  `Unsupported` outside its crate is E0639, and the landing adds `Completion<T>: Send` as E0277.
  The dyn-compatibility pin `const _: fn(&dyn CapabilityProvider<Probe>) = |_| {};` stops
  compiling if the provider trait stops being dyn-compatible.
- The tests of the clipboard landing, listed in §4: copy and paste through the headless
  clipboard; a UI runtime with no provider answers `NotRegistered`, not a panic; a provider
  answering `NotOnThisPlatform` reaches the widget unchanged; two plugins for the clipboard, and
  two capabilities with one `NAME`, fail before a window opens; a result the OS delivers on a
  thread other than the owner's reaches the widget at an owner turn boundary, through the poster;
  the trybuild fixtures above; and a package fixture that depends only on `flui-sdk` and the
  contract, and a leaf-crate fixture that depends on the contract alone, each declaring,
  registering and acquiring its own capability.
- The Android provider's tests hold the rule that a permission check never returns
  `Denied(Permanently)` (§2, item 8).
- ADR-0097's gate (`cargo heli xtask globals`) keeps the single trampoline cell the only
  process-global entry of a provider in the repository (§2, item 9); every provider meets
  the rule as a conformance requirement (ADR-0154).
- `a_dropped_handle_ends_its_subscription`: the scripted provider records the subscription ending
  when the last clone of a handle drops (§2, item 3). That long-lived OS work is an application
  service and not a widget-held handle is a row of each capability conformance table (ADR-0154),
  not of the seam.
- §5: `reads_asked_while_one_is_in_flight_share_its_answer` (two widgets in two windows of one
  runtime: one host read for a burst, every reader holding the same allocation, a later read
  asking again),
  `each_runtime_asks_for_its_own_read` (one provider, two runtimes: a read each, and the retired
  one strands nobody),
  `a_read_after_a_write_never_joins_the_read_before_it`,
  `a_primary_read_after_a_release_never_joins_the_read_before_it`,
  `a_failed_read_reaches_every_reader_that_joined_it` (a failure is no empty clipboard) and
  `an_abandoned_read_abandons_its_joiners_and_the_next_read_asks_again`, in the `capability_seam`
  table of `flui-testing`; `runtime_locals_keep_one_value_per_type_until_retirement`
  (`flui-platform`: one value per type, every slot dropped at retirement though one panics, the
  first failure returned, nothing kept after);
  `a_wake_that_panics_mid_fan_out_leaves_the_other_readers_answered` (`flui-interaction`).
- §6: `a_concealed_write_reaches_the_host_marked` (the `capability_seam` table of
  `flui-testing`: the mark reaches the backend, an ordinary write clears it, the handle answers
  the backend's `Concealment`); the scenario on each host:
  `a_concealed_write_stays_out_of_the_headless_history`,
  `a_concealed_write_offers_the_password_manager_hint` (X11 under Xvfb: `TARGETS` offers the hint
  and it converts to `secret`, an ordinary write offers neither),
  `the_selection_crosses_between_clients` (Wayland, with `a_concealed_source_offers_and_answers_the_hint`
  for the offer and its answer), `a_concealed_write_is_an_ordinary_one_on_the_web`
  (the expected failure), and `a_concealed_write_stays_out_of_the_windows_history` and
  `a_concealed_write_stays_out_of_the_macos_history`, run on those hosts.

## Pros and cons of the options

### 1. The revised seam decided above

- Good, because one generic entry point, `cx.capability::<C>()`, lets the framework, a capability
  crate and a package add a capability the same way (§1 and §2).
- Good, because providers live per UI runtime and a handle is acquired only in a lifecycle hook,
  so a handle that holds an OS subscription has an owner and ends it on drop, with no process
  singleton and no static at the OS-callback boundary (§2, items 2, 3 and 9).
- Good, because `Completion<T>` is the one owner-thread result type for "the OS answers later",
  and an OS callback only posts to the owner (§2, items 5 and 6).
- Good, because `Plugin` and the registrar sit in the contract crate, so a leaf capability crate
  registers without depending on `flui-sdk` or `flui-runtime` (§2, item 4).
- Good, because `ProviderContext` hands out a `WindowRef`, not `PlatformWindow`, so no
  window-trait method becomes semver surface for capability crates (§2, item 6).
- Good, because absence is one constructed value, `Unsupported { reason }`, shared with the window
  contract, and permissions say what was granted (§2, items 7 and 8).
- Good, because the registry is keyed by the marker and not by the implementation, so substitution
  is a provider override (§1).
- Good, because the rules are checked by the compiler: E0599, E0277 and E0639 (§3).
- Bad, because a widget still cannot take a capability in `build`, so long-lived OS work becomes
  an application service.
- Bad, because the additions of this revision have not been built yet, and the permission types
  wait for the first capability that needs a permission (§3).
- Bad, because the clipboard landing removes `clipboard_handle`, the synchronous `Clipboard` trait
  and the contract's `InMemoryClipboard`, and later removes `LifecycleContext::storage`
  (§2, items 10 and 11).

### 2. Accept ADR-0084 unchanged, amend later

- Good, because its open seam, per-runtime registry, precedence and refusal of `inventory` carry
  over (§1).
- Bad, because its provider needs a window.
- Bad, because it caches handles, and a cached location handle keeps GPS on.
- Bad, because it hands out the provider trait object.
- Bad, because it leaves three methods unclassified.
- Bad, because its `Plugin` placement forces every capability crate onto `flui-sdk`.

### 3. A synchronous `Clipboard` trait object as the handle, with an asynchronous twin (GPUI)

- Bad, because `read_text` cannot be implemented honestly on web and Wayland.
- Bad, because it is two shapes for one fact.

### 4. A process singleton per service (Flutter's static instance, Capacitor's registry)

- Bad, because it cannot serve several UI runtimes.
- Bad, because it invites statics at the OS-callback boundary, which is where the references'
  defects are.

### 5. The handle type as the key (Tauri's `state::<T>()`)

- Bad, because it names the one concrete implementation, so substitution means replacing the
  plugin.

### 6. Twin blocking and asynchronous APIs (Tauri)

- Bad, because it is two shapes for one call, and the response path deadlocks; AGENTS.md lists
  re-entry with guards held first among the recurring defects.

### 7. `async fn` in handle traits as the base

- Good, because it is right as the adapter outside the frame path (`Completion::into_future`).
- Bad, because it needs an executor on the owner thread and is not dyn-compatible without boxing,
  which makes it wrong as the base.

### 8. Callbacks only

- Bad, because they are poor for the multi-step permission flow.

### 9. Effects as data (Crux; [redbadger/crux@1ef0f547e6 crux_core/src/testing.rs:8-12](https://github.com/redbadger/crux/blob/1ef0f547e6/crux_core/src/testing.rs#L8-L12))

- Bad, because it needs the application to be a pure function.

### 10. The whole `PlatformWindow` in `ProviderContext`

- Bad, because every method a capability crate names becomes semver surface for all of them.
- Bad, because portals need only a raw handle.

### 11. One `LifecycleContext` method per capability

- Bad, because packages can never add one.

### 12. Capabilities as inherited views, readable anywhere (Compose, GPUI)

- Bad, because they are readable in `build`, which ADR-0078 forbids for capabilities.
- Bad, because an OS subscription would lose its owner.

### 13. Tauri-style permission files in the seam

- Bad, because they isolate access to a webview's IPC layer
  ([tauri-apps/tauri@9318ab84b8 crates/tauri-utils/src/acl/capability.rs:77](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-utils/src/acl/capability.rs#L77)), FLUI has no
  untrusted code inside the app, and OS permission is per capability and per run.

## More information

- **Not implemented.** The seam merges together with its first consumer, the clipboard; no part of
  it reaches `main` unwired (§4). Storage follows as the second built-in capability and
  `flui-system` (ADR-0154) as the first capability crate. The permission types enter the contract
  with the first capability that needs a permission, notifications or location, and not with the
  clipboard (§2 and §3). The additions of the 2026-10-20 revision have not been built; the
  first landing builds them (§3).
- **Revisit when.** The idea of Tauri-style permission files may return for the agent protocol
  (ADR-0095), not for the seam. The spelling of a new name in §3 may change in detail at
  the first landing while the properties of §2 hold.
- **Prototype.** The shape of 2026-10-06 was built and run in a scratch workspace on rustc 1.99.0.
