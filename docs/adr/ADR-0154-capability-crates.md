# ADR-0154: Capability crates carry their own OS code

- **Status:** Accepted (2026-10-20). Not implemented: the workspace rule, the `flui-cli` support
  and the first crate, `flui-system`, land together.
- **Date:** 2026-10-06
- **Revised:** 2026-10-20 — after reading how Tauri, Flutter, Capacitor, Expo, Bevy and the Rust
  mobile crates ship platform services (sources below): the carve-out for handles that framework
  widgets name, the native-code tiers, declarations, the host bridge, testing, the line between the
  core host and capability crates, and the first crate's contents.
- **Supersedes:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §3 in part (the kinds
  gain `capability`) and §2 in part (the reach table gains its row);
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) in part (official packages keep their
  rules; capability crates are a second kind of package with rules of their own).
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) (the seam this record builds on),
  [ADR-0153](ADR-0153-release-train-and-capability-crates.md) (capability crates ship in lockstep
  with the train until the contract has its own version),
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §3 (the core host's
  closed list), [ADR-0247](ADR-0247-android-java-view.md) (the embedded dex recipe),
  [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (the Android and iOS hosts the bridge reaches),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md),
  [ADR-0243](ADR-0243-one-testing-kit-per-layer.md),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md)
- **Sources:** other projects are read at pinned commits and cited as `owner/repo@sha path:line`:
  Tauri v2 (`tauri-apps/tauri@9318ab84b8`) with its plugins
  (`tauri-apps/plugins-workspace@11211c30e7`), Flutter (`flutter/flutter@6597889f57`) with its
  packages (`flutter/packages@1383427519`, `Baseflow/flutter-permission-handler@0684469188`),
  Expo (`expo/expo@0937842df7`), Project Robius (`project-robius/robius-location@1974355613`),
  `rust-mobile/ndk-context@0eb252bf23`, `bilelmoussaoui/ashpd@c9b9ddcf4a`, the XDG portals
  (`flatpak/xdg-desktop-portal@49075617ff`) and the Cargo book (`rust-lang/cargo@d4a2c5853b`).

Crate names follow ADR-0151: the contract crate is `flui-platform` (today `flui-platform-api`) and
the core host is `flui-native` (today `flui-platform`).

## Context and problem statement

Toward 1.0, FLUI needs the services applications expect from a platform: location, sensors,
camera and media, notifications, secure storage and biometrics, device state, sharing, file
dialogs, launchers and the tray. Each needs OS code. Today OS code may live only in the core host
(`allowed-dependents = ["flui-app"]`), and the `pkg` reach table forbids `windows-*`, `objc2-*`,
`jni-*`, `ndk-*` and `android-*`. Putting every service in the core host would make it a monolith
released with the train, put every service's trait in the Stable contract, and leave third
parties no way to add one.

Other frameworks agree: the core owns what every application needs; an optional service is its
own package with its own OS code (Flutter federated plugins, Tauri plugins, Bevy plugins, Slint
backends). Flutter's per-OS federation exists because Dart has no conditional compilation; in
Rust one crate with `cfg`-gated platform code does that job, and a replacement implementation is a
provider override through the seam. What federation also does is put the interface where the
consumers are and the implementation where the OS code is; that part is dependency inversion, and
FLUI keeps it where framework widgets need a handle (decision 3).

The references also show where this goes wrong, and each point below answers one of them:

- Declarations left to documentation, so apps ship with a missing usage string: Flutter's
  permission handler requires the `Info.plist` keys to be added by hand
  ([Baseflow/flutter-permission-handler@0684469188 permission_handler/README.md:60-83](https://github.com/Baseflow/flutter-permission-handler/blob/0684469188/permission_handler/README.md#L60-L83)), and
  Project Robius documents the manifest keys in a doc comment
  ([project-robius/robius-location@1974355613 src/lib.rs:3-16](https://github.com/project-robius/robius-location/blob/1974355613/src/lib.rs#L3-L16)).
- Manifests patched from build scripts, which is environment-dependent and reaches direct
  dependencies only ([tauri-apps/tauri@9318ab84b8 crates/tauri-utils/src/build.rs:121](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-utils/src/build.rs#L121),
  [crates/tauri-plugin/src/build/mobile.rs:38-96](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-plugin/src/build/mobile.rs#L38-L96); the Cargo book: "metadata is only passed to
  immediate dependents", [rust-lang/cargo@d4a2c5853b
  doc/book/src/reference/build-scripts.md:482](https://github.com/rust-lang/cargo/blob/d4a2c5853b/doc/book/src/reference/build-scripts.md#L482)).
- `javac` or `swiftc` run from `build.rs`, which puts a JDK or Xcode on every consumer's machine
  ([project-robius/robius-location@1974355613 build.rs:15-50](https://github.com/project-robius/robius-location/blob/1974355613/build.rs#L15-L50); Tauri's Swift build,
  [crates/tauri-utils/src/build.rs:22-36](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-utils/src/build.rs#L22-L36)).
- Process statics at the OS-callback boundary ([tauri-apps/tauri@9318ab84b8
  crates/tauri/src/plugin/mobile.rs:32-35](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/plugin/mobile.rs#L32-L35); [rust-mobile/ndk-context@0eb252bf23
  ndk-context/src/lib.rs:28](https://github.com/rust-mobile/ndk-context/blob/0eb252bf23/ndk-context/src/lib.rs#L28)).
- A desktop fallback that returns zeros while the support table says `none`
  ([tauri-apps/plugins-workspace@11211c30e7 plugins/geolocation/src/desktop.rs:25-31](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/src/desktop.rs#L25-L31),
  [plugins/geolocation/Cargo.toml:16-21](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/Cargo.toml#L16-L21)).
- Monolithic permission plugins that cannot strip the code they do not use: Flutter's must
  translate `Info.plist` keys into compile flags, and merged flavours risk an App Store rejection
  ([Baseflow/flutter-permission-handler@0684469188 permission_handler/README.md:119](https://github.com/Baseflow/flutter-permission-handler/blob/0684469188/permission_handler/README.md#L119), `ITMS-90683`).

The question this record answers: where does the OS code of an optional platform service live, and
how does an application declare and build it, so that the core host stays bounded and a third party
can add a service the way the framework does?

## Decision drivers

- Only the core host may carry OS code today (`allowed-dependents = ["flui-app"]`), and the `pkg`
  reach table forbids `windows-*`, `objc2-*`, `jni-*`, `ndk-*` and `android-*`.
- Putting every service in the core host would make it a monolith released with the train, put
  every service's trait in the Stable contract and leave third parties no way to add one.
- The core owns what every application needs; an optional service is its own package with its own
  OS code, and the interface sits where the consumers are.
- A handle that framework widgets name keeps its marker in the lowest crate that names it.
- Declarations (permissions, usage strings, background modes) must be checked, not left to
  documentation, and a manifest is not patched from a build script, which depends on the
  environment and reaches direct dependencies only.
- No JDK, Android build tools or Xcode on a consumer's machine: no `javac`, `d8` or `swiftc` from a
  `build.rs`.
- A monolithic permission plugin cannot strip the code it does not use; in Rust an unlinked crate
  and a disabled feature produce no code.
- No process-global state at the OS-callback boundary (ADR-0097), and shared code goes down, not
  sideways: the host owns the `JavaVM`, the Activity and the window handle.
- Claimed support is executed support, and a provider never fakes success.
- The contract rides the release train until it has its own version (ADR-0153), so release work
  grows with the number of capability crates.

## Considered options

1. The capability kind decided below: leaf crates per OS permission set, strict declarations
   applied by `flui-cli`, a host bridge through the provider context, and a simulated provider with
   a conformance table in each crate.
2. All OS code in the core host.
3. One `flui-services` crate with a feature per service.
4. Flutter-style federation (interface, per-OS implementations, app-facing package).
5. Allow OS crates in official packages.
6. Menus and incoming activation as capability crates.
7. Patching manifests from a build script (Tauri).
8. Declarations in documentation only.
9. `javac`, `d8` or `swiftc` from `build.rs` (Robius, Slint, Tauri).
10. Autolinking through `links` and `DEP_*` variables (Tauri).
11. A shared "capability common" crate for the Android bridge.
12. A capability crate depending on `flui-testing` for its simulated provider.

## Decision

Chosen option: 1, because it keeps the core host bounded by the closed list of ADR-0151 and gives
each optional service a leaf crate with its own OS code, strict declarations that `flui-cli`
applies, and a host bridge that holds no process-global state, at the price of release work that
grows with the number of crates.

### 1. A new kind

`tier-kind = "capability"` in tier `pkg`. A capability crate:

- depends normally only on the contract crate, which carries the seam types, `Plugin` and the
  registrar (ADR-0152), so registering needs neither `flui-sdk` nor `flui-runtime`; on external
  crates; optionally on `flui-sdk`, only if it ships widgets; and, for a capability whose marker
  a framework crate owns, on that crate (decision 3);
- may reach OS crates: its reach table is the `pkg` table without the OS-crate globs, and it
  still forbids `winit`, `wgpu`, `tokio`, the core host, `flui-app` and `flui-engine`;
- is a **leaf** below the facade: no FLUI crate other than the facade, which re-exports it behind
  a feature (§8), and no other capability crate depends on it normally;
- is released with the train while the contract rides it (ADR-0153), and may set its own version
  once the contract has one;
- registers through the seam with a `Plugin` and ships a simulated provider and a conformance
  table that every platform it claims must pass (decision 10);
- keeps no process-global state (ADR-0152). ADR-0097's gate scans the workspace, so it enforces
  the rule for crates in the repository; for any other crate it is a stated requirement.

### 2. One crate per OS permission set and release cadence, not per API

Services that share an OS permission and are used together share a crate (for example
accelerometer, gyroscope, magnetometer and barometer). A monolithic plugin has to translate
manifest keys into compile flags to keep an unused permission API out of the binary (Flutter's
`permission_handler` does, to avoid an App Store rejection); in Rust an unlinked crate and a
disabled feature produce no code, so the split costs nothing. `flui-system` is introduced by this
ADR (decision 11); each later crate is introduced by its own ADR naming its permission set and
its claimed platforms.

### 3. Handles that framework or package widgets name

The leaf rule describes the OS provider. When framework or package widgets must name a handle
(the clipboard, opening links), its marker and handle stay in the lowest crate that names it,
exactly as ADR-0152 does for the clipboard, and only the OS provider lives in the capability crate
(or in `flui-app`, the composition root, when the service is on the core host's closed list: the
marker sits above the core host, which cannot name it). The provider crate depends on the crate
that owns the marker; nothing depends back on it. This applies to the few capabilities with
in-framework consumers; it needs no per-OS package, because `cfg` still selects the platform code. Haptics
is not among them: it is a facet of the window (ADR-0210, ADR-0254) and does not go through the
seam.

The clipboard and storage are the services the framework needs before any capability crate
exists, so their providers are built-ins of `flui-app`, over the OS objects the core host exposes
(ADR-0152).

### 4. Where the line falls between the core host and capability crates

ADR-0151 §3 holds the closed list and the rule behind it: a service is core when delivering it
needs an object that only the host owns (the application delegate, the Activity, the window
procedure, the event loop), or when the framework's own widgets consume it. A capability crate
holds what is permission-gated, hardware-bound, or something a supported platform legitimately
lacks, and everything else the rule leaves out.

- **The application menu is core**: the menu model, the menu bar, context menus, and the Dock menu,
  which the application delegate hands out. A macOS application without a menu bar has no Quit or
  Edit menu (the App menu bar row of `docs/platform-parity.md`), and the references keep it in
  core: Tauri through `muda` ([tauri-apps/tauri@9318ab84b8 crates/tauri/Cargo.toml](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/Cargo.toml)), Flutter's
  `PlatformMenuBar` ([flutter/flutter@6597889f57
  packages/flutter/lib/src/widgets/platform_menu_bar.dart](https://github.com/flutter/flutter/blob/6597889f57/packages/flutter/lib/src/widgets/platform_menu_bar.dart)), GPUI's `Platform::set_menus`
  ([zed-industries/zed@fae8a35482 crates/gpui/src/platform.rs:488](https://github.com/zed-industries/zed/blob/fae8a35482/crates/gpui/src/platform.rs#L488)) and Electron's `Menu`
  ([electron/electron@30db518815 docs/api/menu.md](https://github.com/electron/electron/blob/30db518815/docs/api/menu.md)).
- **The Windows jump list is not core**: `ICustomDestinationList` needs no object the host owns.
  It lives in `flui-system`.
- **Incoming URL and file activation is core**: delivery at cold and warm start, reopen, and
  handing arguments to a single instance. The consumer is the router.
- **The tray is a capability**, desktop only, and it enters `flui-system` together with the menu
  feature: it builds on the menu model, which is core, and without that model a tray is only an
  icon.
- **Opening a URL, a file or a folder in the system's handler is a capability**; a `Link` widget
  names its handle through decision 3.

No capability crate claims a menu or incoming activation.

### 5. Native code, in order of preference

1. **Rust only**, over `objc2-*`, `windows`, `zbus` and `ashpd`, and `jni` wrapped at the bridge
   (decision 9). No new toolchain. This is the default.
2. **A checked-in dex**, for Java that needs only framework APIs (no AndroidX, Play Services or
   resources): ADR-0247's recipe, generalised. The Java sources and the dex live in the crate, a
   record file holds the digests of both, a byte-for-byte gate checks them, and consumers need no
   JDK and no build script. One Java package per crate. `flui-cli` puts the dex in the APK as it
   does FLUI's own (ADR-0255), so the Activity's class loader loads it.
3. **A Gradle library module** in the crate (an `android/` directory), or **an SPM package**,
   consumed by the project `flui-cli` generates, when the capability needs AndroidX, Play Services,
   a manifest component, resources or a foreground service. The crate's platform table says
   "Android (Gradle)" so the cost is visible.

Never `javac`, `d8` or `swiftc` from a `build.rs`. It puts a JDK and Android build tools, or Xcode,
on every consumer's machine and makes the build depend on their versions (ADR-0247 rejected it for
the same reason); Tauri's own Swift build carries a comment that its environment handling can be
accessed from multiple threads ([tauri-apps/tauri@9318ab84b8 crates/tauri-utils/src/build.rs:22-36](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-utils/src/build.rs#L22-L36)).

### 6. The linked set comes from `cargo metadata`

`flui-cli` finds the capability crates an application links by reading `cargo metadata`, which
lists every resolved package with its `[package.metadata]` and its enabled features, transitive
dependencies included. It does not use `DEP_*` build-script variables, which reach only a direct
dependent, and no capability crate patches a manifest from a build script. `links` is for a crate
that exports a global native symbol; a capability crate that carries its state in the native
object (ADR-0152) exports none and needs neither `links` nor a `build.rs`.

### 7. Declarations are strict, feature-gated Cargo metadata

A capability crate declares what it needs from the application's manifests in
`[package.metadata.flui.capability]`: strict, like the existing `[package.metadata.flui]` keys,
so an unknown key is an error. A Cargo feature of the crate carries both its code and its extra
declarations, so a disabled feature removes both; a build that lacks a declaration the capability
needs makes the provider answer `NotDeclared { what }` (ADR-0152) rather than letting the OS refuse
or end the process. The schema below is illustrative; its keys are fixed with the first crate that
needs a declaration.

```toml
# Illustrative, not a schema decision.
[package.metadata.flui.capability]
name = "location"                                    # equals Capability::NAME

[package.metadata.flui.capability.android]
permissions = ["android.permission.ACCESS_COARSE_LOCATION"]

[package.metadata.flui.capability.apple.info-plist]
NSLocationWhenInUseUsageDescription = { default = "Allow $(PRODUCT_NAME) to use your location" }

[package.metadata.flui.capability.features.background.android]
permissions = ["android.permission.ACCESS_BACKGROUND_LOCATION"]
[package.metadata.flui.capability.features.background.apple]
background-modes = ["location"]
```

- **`flui-cli` applies them.** It walks `cargo metadata` including enabled features and writes
  the Android manifest entries (or leaves them to Gradle's merger for tier 3 of decision 5), the
  `Info.plist` usage strings, the entitlements, the privacy manifest and, for a packaged app, the
  MSIX capabilities. It writes them into regions it owns, delimited by markers (ADR-0255), so a
  file the application also edits keeps the application's own lines. Static linking means Apple's
  per-SDK privacy bundle does not exist, so the application carries the union; that aggregation
  rule is read from the references, not from Apple's text, and is checked when the generator
  lands.
- **A verify mode** (a `flui doctor` check) reads the same data for a project the CLI does not own
  and lists the keys it lacks.
- **The application overrides.** An application-side table replaces a usage string or turns a
  capability's part off, as Expo's config plugins do (typed props with defaults, and `false`
  removes the key and the code behind it: [expo/expo@0937842df7
  packages/expo-location/plugin/src/withLocation.ts:183-193](https://github.com/expo/expo/blob/0937842df7/packages/expo-location/plugin/src/withLocation.ts#L183-L193)); the default is the crate's.

Build-script patching (Tauri) works only when the CLI sets a variable, only on a macOS host for
iOS and only for direct dependencies, so its output differs by environment
([tauri-apps/tauri@9318ab84b8 crates/tauri-utils/src/build.rs:121](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-utils/src/build.rs#L121),
[crates/tauri-plugin/src/build/mobile.rs:38,81](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-plugin/src/build/mobile.rs#L38)); documentation-only declaration (Flutter's iOS
keys, Electron, Robius) is how applications ship without a usage string.

### 8. `flui add <capability>`

Capability crates reach an application two ways. The facade re-exports each official capability
crate behind a feature of the same name, under one namespace: `flui = { features = ["location",
"bluetooth"] }` gives `flui::services::location::*`, the way `material` and `cupertino` already
sit behind facade features (ADR-0088); the crates stay separate, one per permission set, so a
breaking change in one reaches only the applications that enabled it. And `flui add <capability>`
runs `cargo add`, enables the facade feature, writes the declarations (decision 7) and shows the
registration snippet, as `tauri add` does ([tauri-apps/tauri@9318ab84b8
crates/tauri-cli/src/add.rs](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri-cli/src/add.rs)).

### 9. The host bridge

Shared code goes down, not sideways, and there is no "capability common" crate. The bridge is not
shared code: the `JavaVM` and the Activity, the application delegate and the window handle have
one owner, the host, so a capability crate asks for them through `ProviderContext` (ADR-0152),
which grows by methods and never exposes an OS type in a public signature. In the order the first
crates need it:

1. **`owner()` and `WindowRef`**: a poster to the owner thread and a window's id, scale and raw
   handles.
2. **Android**: the host's `FluiActivity` queue (ADR-0255) provides `launch_for_result` (with
   `flui-system`, whose file picker needs an activity result) and `request_permissions` (with the
   first capability that needs a permission, ADR-0152), each returning a `Completion` and routed
   by a token per call, not by an integer shared by every crate. The `JavaVM` and the current Activity are raw pointers in the
   context, not a `static`, so each crate picks its own `jni` version; the Activity is not stable
   (Android recreates it), so the bridge reports its replacement.
3. **Apple**: events in FLUI's types for what the core host already receives (URL open, reopen,
   scene request), and a new variant only when a crate needs a new selector (a push token, a
   notification response). Registering `objc2` protocol objects is rejected: it leaks types and
   versions into every capability crate.
4. **Win32**: nothing until a capability has a consumer for a message hook.
5. **Linux**: nothing; portals take the raw window handle (`parent_window` as `x11:<XID>` or
   `wayland:<HANDLE>`, [flatpak/xdg-desktop-portal@49075617ff doc/window-identifiers.rst:11-21](https://github.com/flatpak/xdg-desktop-portal/blob/49075617ff/doc/window-identifiers.rst#L11-L21);
   [bilelmoussaoui/ashpd@c9b9ddcf4a client/src/window_identifier/mod.rs:87](https://github.com/bilelmoussaoui/ashpd/blob/c9b9ddcf4a/client/src/window_identifier/mod.rs#L87) builds it from
   `raw-window-handle`).

### 10. Testing, and claimed support

- **A scripting primitive in the contract crate's `scripted` feature** (not `testing`: ADR-0243
  keeps that name for test support, and `flui-app` uses the primitive under `driver`): an ordered
  script of responses to prompts, service toggles, revocations and injected errors, plus
  virtual-time event streams, all resolved at an owner turn boundary. It sits in the contract
  because a capability crate may not depend on `flui-testing`, and because every call the OS
  answers later is a `Completion` that a script can drive.
- **Every capability crate exposes `testing` with a simulated provider** built on that primitive.
- **One conformance table per crate**, run against the simulated provider in CI and against each
  real platform where a device or an OS runner exists.
- **Claimed support is executed support.** The platform table lists a platform as supported only
  when its conformance table has run on that platform in CI or in a recorded local run; otherwise
  "type-checked".
- **A provider never fakes success.** It answers `Unsupported` instead, and a capability whose
  guarantee can degrade (secure storage without a secret store, a clipboard without a system
  clipboard) reports the level on its handle.

### 11. The first crate and the ones after it

**`flui-system`** is the first capability crate. It holds the services of the OS shell that the
core host's closed list leaves out:

- file and folder dialogs and the message box;
- open a URL, open a file, reveal in the file manager, move to the trash;
- share;
- launch at login;
- global shortcuts;
- taskbar progress and badge;
- the Windows jump list;
- OS paths and OS information;
- the tray, which enters together with the menu feature (decision 4).

`open_url`, `reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path` and `app_path`
leave the core host's `Platform` for it.

`flui-notify`, `flui-device`, `flui-vault` and `flui-location` follow as later crates, each by its
own ADR (notifications, device state and sensors, secure storage and biometrics, location).

### 12. The rule lands with its first user

`cargo heli xtask workspace` gains the kind rules (allowed dependencies, leaf, version) and
`cargo heli xtask reach` the table, each with a `--self-test` case that plants a violation, in the pull
request that adds `flui-system`. The declarations, the verify mode and `flui add` land with the
first capability crate that needs them.

### Consequences

- Good, because the core host stays bounded by ADR-0151's closed list, which includes the
  application menu and incoming activation.
- Good, because an application compiles and declares permissions only for the services it uses.
- Bad, because release work grows with the number of capability crates, each published with every
  train until the contract has its own version (ADR-0153); changelog fragments and automated
  version bumps keep it per crate.
- Good, because an application finds every official capability under one namespace,
  `flui::services::<name>`, behind a facade feature, while the crates stay separate and a breaking
  change in one reaches only the applications that enabled it (decision 8).
- Bad, because a capability whose handle framework widgets name costs one more crate boundary (the
  marker below, the provider in the leaf).
- Good, because in exchange `flui-material` and `flui-interaction` can name the clipboard and links
  without depending on OS code.
- Bad, because the Gradle and SPM tier means an application that links such a crate needs the
  Gradle or Xcode project `flui-cli` generates; the platform table says so.

### Confirmation

- `cargo heli xtask workspace` gains the kind rules (allowed dependencies, leaf, version) and
  `cargo heli xtask reach` the table, each with a `--self-test` case that plants a violation, in
  the pull request that adds `flui-system` (decision 12).
- ADR-0097's gate (`cargo heli xtask globals`) scans the workspace and so enforces the
  no-process-global-state rule for capability crates in the repository (decision 1).
- The Java sources and the checked-in dex of a tier 2 crate are held by a record file of digests
  and a byte-for-byte gate (decision 5).
- A declaration the capability needs and the build lacks makes the provider answer
  `NotDeclared { what }`; an unknown key in `[package.metadata.flui.capability]` is an error; the
  verify mode (a `flui doctor` check) lists the keys a project lacks (decision 7).
- One conformance table per crate, run against the simulated provider in CI and against each real
  platform where a device or an OS runner exists; the platform table lists a platform as supported
  only when its conformance table has run on that platform in CI or in a recorded local run,
  otherwise "type-checked" (decision 10).
- Checked when the generator lands: the aggregation rule for Apple's per-SDK privacy bundle, which
  is read from the references and not from Apple's text (decision 7).
- `cargo xtask android-dex` extends to every `build.rs` of the workspace: a build script that names
  `javac`, `d8` or `swiftc` fails it (decision 5); the `capability` kind rule of `cargo xtask
  workspace` refuses a declaration that claims a menu or incoming activation (decision 4). A crate
  outside the repository is not gated: its conformance table (decision 10) is the only check it
  runs (decision 1).

## Pros and cons of the options

### 1. The capability kind decided above

- Good, because the core host stays a bounded set of what every application needs (decision 4)
  and an optional service is a leaf crate with its own OS code that registers through the seam
  (decision 1).
- Good, because one crate per OS permission set means an application links and declares only what
  it uses; in Rust an unlinked crate and a disabled feature produce no code (decision 2).
- Good, because the framework-widget carve-out keeps a handle's marker where the consumers are and
  puts only the OS provider in the capability crate (decision 3).
- Good, because Rust first, then a checked-in dex, then a Gradle or SPM module, and never a
  toolchain run from `build.rs`, keep a JDK or Xcode off consumers' machines (decision 5).
- Good, because declarations are strict, feature-gated Cargo metadata that `flui-cli` reads from
  `cargo metadata` and applies, so a missing usage string is detectable before the OS refuses or
  ends the process (decisions 6 and 7).
- Good, because the host bridge hands out the `JavaVM`, the Activity and window handles through
  `ProviderContext`, never a `static` (decision 9).
- Good, because every crate ships a simulated provider and a conformance table, and claimed support
  is executed support (decision 10).
- Bad, because release work grows with the number of capability crates, and a handle that
  framework widgets name costs one more crate boundary.
- Bad, because the Gradle and SPM tier needs the project `flui-cli` generates.

### 2. All OS code in the core host

- Bad, because it is a monolith released with the train.
- Bad, because every service's trait lands in the Stable contract.
- Bad, because there are no third-party services.

### 3. One `flui-services` crate with a feature per service

- Bad, because one breaking change in one service breaks every user.
- Bad, because feature combinations multiply.
- Bad, because the manifest carries every OS dependency.

### 4. Flutter-style federation (interface, per-OS implementations, app-facing package)

- Good, because the interface sits where the consumers are and the implementation where the OS
  code is; FLUI keeps that part where framework widgets name the handle (decision 3).
- Bad, because it adds up to three packages per service where `cfg` already replaces the per-OS
  ones; the interface split is kept only where framework widgets name the handle, so the crate
  count grows by those few, not by three per service.

### 5. Allow OS crates in official packages

- Bad, because official packages are UI libraries on the SDK, and mixing OS code into them couples
  UI releases to OS work.

### 6. Menus and incoming activation as capability crates

- Bad, because the runtime and every macOS application need them from the first window, and the
  Dock menu needs the application delegate.
- Bad, because Tauri, Flutter, GPUI and Electron keep menus in core, and Tauri delivers activation
  as `RunEvent::Opened` ([tauri-apps/tauri@9318ab84b8 crates/tauri/src/app.rs:220-295](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/src/app.rs#L220-L295)).

### 7. Patching manifests from a build script (Tauri)

- Bad, because it runs only when the CLI sets a variable, only on a macOS host for iOS, and only
  for direct dependencies.
- Bad, because it breaks reproducibility and cargo's rerun model.

### 8. Declarations in documentation only

- Bad, because applications ship with a missing usage string.
- Bad, because an undeclared permission cannot be detected.

### 9. `javac`, `d8` or `swiftc` from `build.rs` (Robius, Slint, Tauri)

- Bad, because it puts a JDK, build tools or Xcode on every consumer's machine, for bytes that
  change only with the sources
  ([slint-ui/slint@277e7676ce internal/backends/android-activity/build.rs:30-79](https://github.com/slint-ui/slint/blob/277e7676ce/internal/backends/android-activity/build.rs#L30-L79)).

### 10. Autolinking through `links` and `DEP_*` variables (Tauri)

- Bad, because it reaches direct dependents only.
- Bad, because it needs a build script in every crate.

### 11. A shared "capability common" crate for the Android bridge

- Bad, because the Activity has one owner, the host; a global crate means a `static mut`
  (`ndk-context`).

### 12. A capability crate depending on `flui-testing` for its simulated provider

- Bad, because it breaches the dependency rule; the scripting primitive lives in the contract's
  `scripted` feature.

## More information

- **Not implemented.** The workspace rule, the `flui-cli` support and the first crate,
  `flui-system`, land together (decision 12); the declarations, the verify mode and `flui add`
  land with the first capability crate that needs them.
- **Open until the first crate that needs it.** The schema in decision 7 is illustrative, and its
  keys are fixed with the first crate that needs a declaration; Win32 has nothing in the host
  bridge until a capability has a consumer for a message hook (decision 9, item 4); each later
  crate (`flui-notify`, `flui-device`, `flui-vault`, `flui-location`) is introduced by its own ADR
  naming its permission set and its claimed platforms (decisions 2 and 11).
