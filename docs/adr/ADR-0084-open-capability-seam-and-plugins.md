# ADR-0084: Platform capabilities are an open, typed set registered by plugins

- **Status:** Proposed
- **Date:** 2026-09-25
- **Revised:** 2026-09-26 (prototype of the seam; see Context)
- **Supersedes in part (on acceptance):** [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  §1, the sentence "A new capability is a method on `LifecycleContext`, never on
  `BuildContext`" (for platform capabilities; the build/lifecycle split and the sealing stand)
- **Amends (on acceptance):** [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md)
  §4 (the widget-facing haptics handle arrives through the registry; this replaces the deferral
  "No widget-facing handle exists yet"); [ADR-0038](ADR-0038-data-transfer-architecture.md) §7
  (`DataTransferHandle` is reached through the registry, not a `LifecycleContext` method)
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) ("Platform-adaptive
  behavior is a capability seam, not a branch"),
  [ADR-0037](ADR-0037-presentation-ownership-domains.md),
  [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §6,
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) (§3: `accessibility()` leaves
  `PlatformWindow`),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0086](ADR-0086-signal-writes-through-event-context.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md)
- **Refs:** decision D3 of the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md);
  index in [`design/decisions.md`](../../design/decisions.md)

## Context

ADR-0078 made capability acquisition a type: `ViewState::init_state` and
`did_change_dependencies` receive `&dyn LifecycleContext`, `build` receives
`&dyn BuildContext`, and a capability acquired in `build` does not compile. `LifecycleContext`
is sealed through its supertrait (`crates/flui-view/src/context/build_context.rs:17-19,106,376-377`)
and carries eleven methods, from `rebuild_handle` to `pipeline_owner` (`build_context.rs:392-565`).
The rule that came with it — "A new capability is a method on `LifecycleContext`" — closes the
set: only `flui-view` can add a capability, and only by naming its type, so a capability whose
backend lives in a third-party crate cannot exist. AGENTS.md's "Extending FLUI" row repeats the
rule.

The platform side already has more than the widget side can reach:

- **Haptics.** `PlatformWindow::haptics()` exists (`crates/flui-platform-api/src/platform_window.rs`),
  and `PresentationState::perform_haptic_feedback` resolves it
  (`crates/flui-runtime/src/presentation.rs:970`), but that method and its forwarder
  `UiRealm::perform_haptic_feedback` (`crates/flui-runtime/src/ui_realm/frame_clock.rs:466`) carry
  `expect(dead_code)` with "no production caller yet" (both moved from `flui-app` by
  ADR-0083). ADR-0031 §4 deferred the widget-facing handle "with the first widget
  consumer, as a lifecycle capability".
- **Clipboard.** `Platform::clipboard()` is a required method
  (`crates/flui-platform/src/traits/platform.rs:423`), resolved once per loop into `AppRuntime`
  (ADR-0038 §9), and the accessor that would hand it out is dead code: "no production caller yet
  -- a Clipboard capability through BuildContext is future wiring"
  (`crates/flui-app/src/app/runtime.rs:1631-1641`).
- **Data transfer.** ADR-0038 §7 names `DataTransferHandle` "a lifecycle capability on
  `LifecycleContext`"; no such method exists in the list above.

Three constraints shape any fix:

- `&dyn LifecycleContext` appears 136 times in `crates`, `src` and `examples`
  (`grep -rn '&dyn LifecycleContext' crates src examples --include=*.rs | wc -l`). A generic
  method `fn capability<C>(&self)` on the trait would make it not object-safe and break every
  one of those sites.
- Rust has no link-time registration without `inventory` or `linkme`, and both carry caveats on
  wasm and static libraries; neither is a dependency today.
- Flutter's federated plugins have no way for an application to replace a plugin's endorsed
  implementation (flutter#80374, cited in the review's
  [ecosystem design](../research/2026-09-25-architecture-review/designs/ecosystem_evolution_first.md)).

### What a prototype showed (2026-09-26)

Branch `spike/capability_seam`, commits `d80215406` and `e2c96d35a`, not merged. The seam and
the registry stood in `crates/flui-view/src/capability/` in place of `flui-platform-api` and
`flui-runtime`.

- **Shown.**
  - `capability_erased` on the sealed trait plus the blanket `LifecycleContextExt` compiled.
    `cargo check --workspace --all-targets --all-features` is green, and the 136 sites are
    untouched (a grep counts 138; the extra 2 are doc lines).
  - Only `ElementBuildContext` and `BuildCtx` implement the method.
  - A `compile_fail,E0599` doctest covers `cx.capability::<Clipboard>()` on
    `&dyn BuildContext`.
  - A crate outside the workspace registered a capability and received it in `init_state`;
    without its plugin it got `NotRegistered`.
  - 3 of 5 external tests fail when `BuildCtx::capability_erased` ignores the scope.
- **Narrower than the success metric.**
  - The fixture depended on `flui-view`, `flui-platform` and `flui-types`, not on
    `flui-platform-api` and `flui-sdk`.
  - Its provider wrapped the in-tree `PlatformWindow::haptics()` and `FakeHaptics`.
  - The tests built the registry by hand. The runner path (`main_window.rs` and the
    `presentation.rs` wiring) and the conflict-at-run path were **not exercised**.
- **Corrected by this revision.**
  - The prototype shared one registry per application through an owner-thread cell
    (`runner/host.rs`). Android, iOS and web realms got an empty one, where even the clipboard
    answers `NotRegistered`.
  - It registered `Cursor`, `TextInput` and `Accessibility` as widget capabilities, against §5.
  - It removed the defaults of `text_input()` and `accessibility()` but kept `Option`.
  - It did not edit macOS. `MacOSWindow::accessibility` exists only under `a11y`
    (`crates/flui-platform/src/platforms/macos/window.rs:926-927`), and `a11y` is off by default
    (`crates/flui-platform/Cargo.toml:300,310`), so a default macOS build fails with E0046. It
    was never compiled for macOS.
  - There are three `cfg`-gated `accessibility` overrides, not two: Windows
    (`windows/window.rs:707-708`), winit (`winit/window.rs:324-325`) and macOS. Their `None`
    without `a11y` was intended behaviour.
  - `build()` walked a `HashMap`, so which of several conflicts it reported varied between runs.
  - Its `flui-view → flui-platform` edge is not carried forward; ADR-0082 §2 forbids it.

## Decision

### 1. Two kinds of capability

- A **framework capability** is implemented by the framework itself: `rebuild_handle`,
  post-frame handles, `focus_manager`, keep-alive, `hit_test_handle`, `text_input_handle` (the
  presentation's IME route, ADR-0037 §5), `pipeline_owner`, and `WriterSource` (ADR-0086). These
  stay methods on `LifecycleContext`, as ADR-0078 decided.
- A **platform capability** is an OS service with a backend outside `flui-view`: clipboard, data
  transfer, haptics, file dialogs, camera, share sheets and the like. These are **never**
  methods on `LifecycleContext`. They form an open set, declared by any crate and reached through
  one object-safe method. Platform capabilities come in two classes, core-required and optional
  (§5); both are reached through the same door, `cx.capability::<C>()`.

### 2. The seam

In `flui-platform-api` (ADR-0082), so that a plugin's interface crate needs nothing else:

```rust
pub trait PlatformCapability: 'static {
    /// The value a widget holds; cheap to clone, owner-thread.
    type Handle: Clone + 'static;
    /// Stable name, for errors, logs and the protocol's capability listing.
    const NAME: &'static str;
}

pub trait CapabilityProvider<C: PlatformCapability>: 'static {
    fn provide(&self, window: &Arc<dyn PlatformWindow>) -> Result<C::Handle, Unsupported>;
}
impl<C: PlatformCapability, F> CapabilityProvider<C> for F
where
    F: Fn(&Arc<dyn PlatformWindow>) -> Result<C::Handle, Unsupported> + 'static,
{ /* .. */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("platform capability `{capability}` is unavailable: {reason}")]
#[non_exhaustive]
pub struct Unsupported { pub capability: &'static str, pub reason: UnsupportedReason }
impl Unsupported {
    pub const fn of<C: PlatformCapability>(reason: UnsupportedReason) -> Self;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedReason { NotRegistered, NotOnThisPlatform, NoWindow }
impl fmt::Display for UnsupportedReason {
    /// "no provider is registered", "not available on this platform", "the window is gone".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { /* .. */ }
}
```

- The provider takes `&Arc<dyn PlatformWindow>` so that a handle can keep a `Weak` to its
  window; a `&dyn PlatformWindow` cannot give one.
- `Unsupported::of::<C>` is the constructor other crates use, because the struct is
  `#[non_exhaustive]`.
- `UnsupportedReason` implements `Display` by hand, because the `thiserror` message formats it
  with `{reason}`; `ProviderOrigin` (§4) does the same for `{first}` and `{second}`.
- The provider names `PlatformWindow`, so the seam lands in `flui-platform-api` no earlier than
  `PlatformWindow` does (ADR-0082 §3).
- The class of §5 is deliberately not encoded on `PlatformCapability`, so a move between classes
  stays a change to the provider only.

In `flui-view`, one hidden, object-safe method on the sealed trait and a blanket extension:

```rust
pub trait LifecycleContext: BuildContext {
    // … the framework capabilities …
    #[doc(hidden)]
    fn capability_erased(&self, id: TypeId, name: &'static str) -> Result<Rc<dyn Any>, Unsupported>;
}

pub trait LifecycleContextExt: LifecycleContext {
    fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported> {
        self.capability_erased(TypeId::of::<C>(), C::NAME).map(|handle| {
            handle
                .downcast_ref::<C::Handle>()
                .expect("BUG: capability registry stored a handle of another type")
                .clone()
        })
    }
}
impl<T: LifecycleContext + ?Sized> LifecycleContextExt for T {}
```

`Unsupported` is defined in `flui-platform-api`, so this adds a new normal edge
`flui-view → flui-platform-api` (tier K to tier C, legal under ADR-0081 §1).

`LifecycleContext` stays sealed, so only `flui-view`'s contexts implement `capability_erased`,
and they forward to the realm. `LifecycleContextExt` is in the prelude. Because the method lives
on `LifecycleContext`, `cx.capability::<Haptics>()` inside `build` is still a compile error, the
same `E0599` ADR-0078 relies on. The 136 `&dyn LifecycleContext` sites are untouched.

### 3. The registry is per realm, a parameter of realm construction

- **The provider table is built once per `Application::run`.** `Application::run` validates the
  plugins and overrides **before the platform starts**; a conflict returns
  `Err(AppRunError::CapabilityConflict(c))` and opens no window. The built-in providers for
  clipboard and data transfer capture the platform's services, so they join the table once the
  platform exists. That step cannot conflict: the built-in key set is fixed and was part of the
  validation.
- **Each realm receives its own `CapabilityRegistry` as a parameter of its construction.** Today
  that is a field of `RealmServices` (`crates/flui-app/src/app/runtime.rs:162`); after ADR-0083
  it is the runtime's realm constructor. The registries share the validated table by `Rc` on the
  owner thread (ADR-0091); handles are cached per presentation; a provider keeps no per-realm
  state. No realm constructor exists without the parameter, so no realm starts with a silently
  empty registry. The table is never a thread-local or a static (ADR-0097), which rules out the
  prototype's owner-thread cell. The table never changes after validation; there is no
  registration at run time. The `Rc` assumes one owner thread; if ADR-0091's per-realm owner
  threads are adopted, the table becomes an `Arc` over `Send + Sync` providers or is built once
  per owner thread.
- **Lookup order.** `NotRegistered` (the table has no provider), then `NoWindow` (the
  presentation's window is gone, and its cache with it), then the provider, which returns a
  handle or its own `NotOnThisPlatform`.
- **A `BuildOwner` with no installed scope answers `NotRegistered` for everything.** That is a
  unit test that mounts without a realm. The guarantee for core capabilities is a property of
  realm construction, and `flui-testing`'s driver builds its realms through it.

### 4. Registration is explicit, one line per plugin

```rust
// flui-runtime, re-exported by flui-sdk
pub trait Plugin: 'static {
    fn name(&self) -> &'static str;
    fn install(&self, registrar: &mut CapabilityRegistrar<'_>);
}
impl CapabilityRegistrar<'_> {
    pub fn capability<C: PlatformCapability>(&mut self, provider: impl CapabilityProvider<C>) -> &mut Self;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderOrigin { BuiltIn, Plugin(&'static str), Application }
impl fmt::Display for ProviderOrigin {
    /// "the built-in providers", "plugin `flui-haptics`", "the application".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { /* .. */ }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error("platform capability `{capability}` is registered by both {first} and {second}")]
pub struct CapabilityConflict {
    pub capability: &'static str,
    pub first: ProviderOrigin,
    pub second: ProviderOrigin,
}

// flui-app
impl<V, F> Application<V, F> {
    /// Collected, and installed in call order.
    pub fn plugin(self, plugin: impl Plugin) -> Self;
    /// An application override.
    pub fn capability<C: PlatformCapability>(self, provider: impl CapabilityProvider<C>) -> Self;
}
// and a new variant of the existing `#[non_exhaustive]` `AppRunError`:
AppRunError::CapabilityConflict(CapabilityConflict)
```

`Plugin` and the registrar are defined in `flui-runtime` and published to package authors
through `flui-sdk` (ADR-0088). An application lists its plugins on the application builder —
`Application::new(factory).plugin(flui_haptics::Plugin)`, a new method beside today's
`with_config` and `on_ready` on `Application` (`crates/flui-app/src/app/application.rs:53,67`).
The registrar registers capabilities only; other runtime hooks, such as ADR-0094's reload hook,
have their own builder method. A plugin chooses its default provider per target with
`[target.'cfg(..)'.dependencies]` in its own manifest.

Precedence:

1. An application override wins over plugins and built-ins, whatever the call order. A later
   override of the same `C` replaces an earlier one.
2. Otherwise the built-in provider, for clipboard and data transfer.
3. Otherwise the one plugin that registered `C`.

Without an override, each of these is a conflict, not a silent last-writer-wins:

- two plugins registering `C`;
- a plugin registering a `C` that has a built-in provider;
- one plugin registering `C` twice.

The table keeps installation order, and the first conflict in that order is the one
`Application::run` reports, so the report is the same on every run.

No `inventory` or `linkme`.

### 5. Two classes: core-required and optional

The owner decided on 2026-09-25 that platform capabilities fall into two classes behind the one
door of §2. The class decides where the backend obligation lives, not how a widget asks.

**Core-required.** Methods of the backend traits in `flui-platform-api`; a backend that lacks one
does not compile. ADR-0038 §9 made `Platform::clipboard()` required so that no backend can forget
it, and rejected a handle with `None` defaults for exactly that reason; this class generalises
that rule. The class holds:

- **clipboard** and **data transfer**: `Platform::clipboard()` and `Platform::data_transfer()`
  (`crates/flui-platform/src/traits/platform.rs:423,435`), already required;
- **cursor**: `PlatformWindow::set_cursor` (`crates/flui-platform-api/src/platform_window.rs`),
  already required;
- **text input and IME**: `PlatformWindow::text_input()`, today a defaulted method returning
  `None` (same file), becomes required;
- **accessibility**: the backend-side window extension trait's `accessibility()` (ADR-0082 §3
  moved it off `PlatformWindow`), today `HostWindow::accessibility()` defaulting to `None`
  (`crates/flui-platform/src/traits/host_window.rs`), becomes required;
- **window chrome basics**: title, size, close request and decorations; the exact method list is
  fixed when the traits move into `flui-platform-api` (ADR-0082).

Core methods return an object, not an `Option`:

```rust
fn text_input(&self) -> Arc<dyn PlatformTextInput>;       // PlatformWindow
fn accessibility(&self) -> Arc<dyn PlatformAccessibility>; // the backend extension trait
```

`InertTextInput` is defined beside `PlatformTextInput`, and `InertAccessibility` beside
`PlatformAccessibility`; both accept every call, report nothing and never panic. A backend or a
build without the service returns the inert object and names the gap in its evidence record: the
gap becomes a line in that backend, not a trait default nobody sees. Today that covers the builds
of Windows, winit and macOS without `a11y`, and text input on Win32, Android, iOS and web, none of
which overrides `text_input` (only headless, macOS and winit do).

The runtime registers built-in widget providers for clipboard and data transfer only. There are
no `Cursor`, `TextInput` or `Accessibility` capability types. Being core-required is a backend
obligation; it does not make the service widget-reachable. The framework consumes those three
itself, each through one route:

- the presentation owns one IME route (`TextInputOwner` through `text_input_handle`, ADR-0037 §5,
  and ADR-0090's pull contract), and a second route would open sessions behind it;
- the semantics host owns the window's one accessibility tree;
- mouse regions resolve the cursor per pointer from hit-testing, and a widget calling
  `set_cursor` would fight them.

Making any of them widget-reachable later is an ADR that amends this section. An application may
override the clipboard and data-transfer providers (§4).

**Optional.** Plugin capabilities registered through the registry (§3, §4); a widget handles a
typed `Unsupported`. Haptics, camera, geolocation, notifications, share sheets and file dialogs
start here. Haptics ships as the first plugin, over the existing `PlatformWindow::haptics()` hook
(`window.rs:342`); `None` from the backend becomes `Unsupported { reason: NotOnThisPlatform }`,
which is ADR-0031's degradation contract made visible: a caller may ignore the error and get
Flutter's silent no-op.

Under a realm, a lookup of clipboard or data transfer returns a handle or `NoWindow`, never
`NotRegistered`. `NotRegistered` arises only for an optional capability the application did not
install, or under a bare owner (§3).

**Which class a capability belongs to.** A capability is core-required when both hold:

1. a framework-owned widget or protocol in the Stable surface needs it to meet a contract FLUI
   states for every supported platform (text entry and paste in a text field, an accessibility
   tree, pointer feedback, a window that can be titled and closed); and
2. every supported platform has an OS service for it, so its absence is a backend defect, not a
   platform fact.

Everything else is optional: a service some supported platform legitimately lacks, or one only
applications use.

**How a capability moves between classes.** Each move is a new ADR that amends this section, and
the capability keeps its type `C`, so `cx.capability::<C>()` call sites do not change.

- *Optional to core-required*, when a Stable-surface widget starts to depend on it and every
  supported backend implements it: the method is added to the backend trait (a breaking change for
  third-party backends, so it ships on a train, never in a patch release), the plugin's provider
  becomes the built-in one, and the plugin stays as a no-op for one train.
- *Core-required to optional*, when a supported platform turns out to lack the service: the method
  leaves the backend trait after one train of deprecation, a plugin takes over the provider, and
  callers that relied on the old guarantee must now handle `Unsupported`, which the ADR lists.

## Alternatives considered

- **A generic method on `LifecycleContext`.** Breaks object safety and all 136 `&dyn` sites.
- **Unsealing `LifecycleContext` so plugins add extension traits with their own storage.** Every
  plugin would need somewhere to keep per-realm state, which pushes it toward a process-global
  or a thread-local, and a context implemented outside `flui-view` could hand out capabilities in
  `build`.
- **Link-time auto-registration (`inventory`, `linkme`).** Registration would depend on which
  object files the linker keeps, with known gaps on wasm and static libraries, and an application
  could not see or order what it installed.
- **Keep adding methods, one per capability, in `flui-view`.** Keeps the set closed; every OS
  service needs a change to the spine, and a third-party plugin cannot exist.
- **Capabilities as `InheritedView`s the plugin inserts above the root.** Reads in `build` would
  become possible, which is what ADR-0078 removed, and an override would depend on tree position.
- **Make every capability optional, including clipboard.** Reintroduces the forgettable wiring
  ADR-0038 §9 rejected.
- **Make every capability core-required.** Every backend, including a minimal embedder, would
  have to implement camera, geolocation and the like, and the set would be closed again: a
  third-party capability could not exist without a change to `flui-platform-api`.
- **Two doors: backend methods for core capabilities, the registry for optional ones.** A move
  between classes would change every call site; with one door it changes only the provider.
- **A registry per application shared through an owner-thread cell** (the prototype). Rejected:
  a realm built on any path that does not fill the cell gets an empty registry silently, as the
  prototype's Android, iOS and web realms did.
- **Register cursor, text input and accessibility as widget capabilities** (the prototype).
  Rejected: each would be a second route beside the framework's own (§5).
- **A class marker on `PlatformCapability`.** Rejected: moving a capability between classes
  would change its type, and with it every call site.

## Consequences

- ADR-0078 §1's closing sentence is narrowed to framework capabilities; AGENTS.md's "Platform
  capability (a new handle)" row in "Extending FLUI" changes in the implementing change to: an
  interface crate on `flui-platform-api`, a provider per target, registration through a plugin,
  a headless fake, and a test that fails without it.
- The dead clipboard accessor (`crates/flui-app/src/app/runtime.rs:1581-1590`) and the haptics
  forwarders (`crates/flui-runtime/src/presentation.rs:970`,
  `crates/flui-runtime/src/ui_realm/frame_clock.rs:466`) get production callers or are deleted in favour
  of the built-in clipboard provider and the haptics plugin.
- **Breaks.** `PlatformWindow::text_input()` returns `Arc<dyn PlatformTextInput>` with no
  default. `accessibility()` moves to the backend extension trait (ADR-0082 §3) and returns
  `Arc<dyn PlatformAccessibility>` with no default. Every backend, including third-party ones,
  implements both; since `PlatformAccessibility` now lives in the internal `flui-semantics`
  (ADR-0082 §2, amended), accepting this clause first re-homes that trait or re-exports it
  through a contract crate. Every `cfg(feature = "a11y")` override gets an inert twin for builds without
  `a11y`, macOS included. The `Option` branches over the bridge
  (`crates/flui-runtime/src/presentation.rs:423`, `:1461` and their siblings) go away.
- `AppRunError` gains `CapabilityConflict`. It is `#[non_exhaustive]`
  (`crates/flui-app/src/app/application.rs:22`), so this is not a break.
- AGENTS.md's "Platform capability" row states the classification rule of §5, so a contributor
  adding a capability knows which class to put it in.
- ADR-0038 §7's `DataTransferHandle` is acquired as `cx.capability::<DataTransfer>()`.
- Registered capabilities are data the realm can list; ADR-0095's protocol can expose that list
  to tools and agents.
- The seam depends on ADR-0082 (for `flui-platform-api`) and ADR-0083 (for the realm's
  registry).
- The runtime's handle cache is per presentation; a presentation that closes drops its handles,
  so a widget that outlives its window sees `Unsupported { reason: NoWindow }` on its next
  acquisition in `did_change_dependencies`.

## Verification

(prototype) marks what `spike/capability_seam` showed; none is merged.

| Test | What it asserts | Why it fails today |
|---|---|---|
| `compile_fail,E0599`: `cx.capability::<C>()` on `&dyn BuildContext`, plus a compiling twin on `&dyn LifecycleContext` (prototype) | The build/lifecycle split holds | No seam |
| `cargo check --workspace --all-targets --all-features` (prototype) | The 136 `&dyn LifecycleContext` sites are unaffected | — |
| An out-of-workspace fixture on **`flui-platform-api` and `flui-sdk` only**: `a_plugin_capability_reaches_init_state`, `without_the_plugin_the_capability_is_not_registered` | A third-party capability is declared, registered through a plugin and acquired in `init_state`; without the plugin it is `NotRegistered`. Still outstanding: the prototype used stand-ins | The crates do not exist |
| `a_capability_conflict_fails_run_before_any_window_opens` (`flui-app`, headless runner) | `Application::run` returns `Err(AppRunError::CapabilityConflict(..))` with `first: Plugin(a)`, `second: Plugin(b)`, and zero windows opened | No plugin API; the prototype's quit path was never run |
| `a_plugin_over_a_built_in_is_a_conflict`, `an_application_override_resolves_a_conflict`, `a_later_override_replaces_an_earlier_one`, `conflicts_are_reported_in_installation_order` | The rules of §4; the last one is deterministic across runs | No registry |
| `every_realm_the_runner_builds_resolves_the_clipboard` (through `Application` and the runner, main and secondary windows) | An `Ok` handle, never `NotRegistered` | No seam; the prototype's runner path was untested |
| `not_registered_is_reported_before_no_window`, `a_closed_window_answers_no_window` | The lookup order of §3 | No seam |
| Haptics: `NotOnThisPlatform` on a window without haptics; `FakeHaptics` on `MockWindow` observes the call; `NotRegistered` without the plugin | ADR-0031's degradation contract | No seam |
| `compile_fail` with a compiling twin: a `PlatformWindow` without `text_input`, and a backend extension impl without `accessibility`, do not compile | The core obligation of §5 | The defaults exist |
| `cargo xtask cross-typecheck` for macOS with default features **and** with `a11y`, shown in the PR | The inert macOS path compiles | The prototype broke it (E0046) |
