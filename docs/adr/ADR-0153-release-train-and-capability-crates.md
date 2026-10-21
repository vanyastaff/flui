# ADR-0153: The release train and capability crates: the contract rides the train until it proves stable

- **Status:** Accepted (2026-10-20). Nothing in this ADR is implemented; it decides what the
  contract and the first capability crates do at 0.2.0 and what moves them to their own versions.
- **Date:** 2026-10-06
- **Revised:** 2026-10-20 — the open question became a decision; the experiments below stay as the
  context for it. A new crate for geometry is not on the table: ADR-0098 dissolved `flui-geometry`
  and `flui-types`.
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md) (geometry is f64 values in
  `flui_foundation::geometry`, no units),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the train guard),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §3 (kinds; only `evolving` sets its own
  version), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0152](ADR-0152-capability-seam-revised.md),
  [ADR-0154](ADR-0154-capability-crates.md)

## Context and problem statement

A capability crate (ADR-0154) depends on the contract crate, which is `flui-platform`
(ADR-0151). The contract depends on `flui-foundation`, which carries the train
guard (`links = "flui_train"`), and pins it exactly (`flui-foundation = "=0.2.0-dev"`,
`crates/flui-platform/Cargo.toml:27`).

### What the contract names from `flui-foundation`

Read on `main` @ `d56188c14`, `rg 'flui_foundation::' crates/flui-platform/src`:

| Item | Where in a public signature | After ADR-0151 |
|---|---|---|
| `geometry::Point` | `PlatformWindow` (positions), `window.rs`, `text_store::session` | stays |
| `geometry::Size` | `PlatformWindow` (sizes), `window.rs` | stays |
| `geometry::Bounds` | `PlatformWindow`, `PlatformTextInput` (cursor area), `text_store::session` | stays |
| `geometry::EdgeInsets` | `PlatformWindow::safe_area_insets` and its callback | stays |
| `geometry::Offset` | `offset_from_coords`, `delta_offset_from_coords` only | moves to the core host (no consumer above it) |
| `DataTransferId` | `DragDropEvent` (part of `PlatformInput`) | stays |
| `ClaimSlot`, `ClaimHandle`, `ClaimOutcome`, `claim_slot` | data-transfer request machinery | moves to the core host with the rest of data transfer |
| `panic::retain_opaque_payload` | text-store machinery, not in signatures | follows the machinery; never in a signature, so not a version concern |

So after ADR-0151 the contract's signatures name four geometry values (`Point`, `Size`,
`Bounds`, `EdgeInsets`, all f64 per ADR-0098) and one identifier (`DataTransferId`).

### Where the train breaks a capability crate

Three cargo experiments in a scratch workspace (rustc 1.99.0; the transcript is in the pull
request). A stand-in `foundation` with `links = "flui_train"`, a `platform` contract that pins it
exactly and re-exports `Size`, a capability crate `location` built against one contract version,
and an application on another.

| Case | Setup | Result |
|---|---|---|
| A | The contract's version moves with the train: `location` needs `platform ^0.2` (foundation `=0.2.0`), the app uses `platform 0.3` (foundation `=0.3.0`) | Resolution fails: "package `foundation` links to the native library `flui_train`, but it conflicts with a previous package" |
| B | The contract keeps its own version: `platform 0.2.1` depends on foundation `=0.3.0`; `location` needs `platform ^0.2`; the app is on train 0.3 | Builds and runs; `location` reads the app's window through the 0.2.1 contract |
| C | No `links` key at all, contract 0.2 vs 0.3 | Fails anyway: E0277, the app's window implements the 0.3 `Window`, `location` takes the 0.2 one |

What breaks a capability crate is **the contract's own version changing incompatibly**, not the
geometry crate as such. Case C shows that removing the train guard would not help: two contract
versions are two trait identities. Case B shows that the train is harmless while the contract's
version stays semver-compatible across trains.

The geometry matters in one way only: an incompatible change to one of the four values above (or
to `DataTransferId`) in a train release forces an incompatible contract release, and with it
every capability crate.

The question this record answers: does the contract's exact pin on `flui-foundation` tie every
capability crate to every FLUI release, and if so, what is the narrowest change that unties it, and
when is it safe to make?

## Decision drivers

- `flui-foundation` carries the train guard (`links = "flui_train"`), and the contract pins it
  exactly, so cargo resolves one train per build graph.
- What breaks a capability crate is the contract's own version changing incompatibly, not the
  geometry crate as such: two contract versions are two trait identities, so removing the guard
  would not help.
- After ADR-0151 the contract's signatures name four geometry values (`Point`, `Size`, `Bounds`,
  `EdgeInsets`, all f64 per ADR-0098) and one identifier (`DataTransferId`) from `flui-foundation`;
  an incompatible change to one of them forces an incompatible contract release.
- A new crate for geometry is not on the table: ADR-0098 dissolved `flui-geometry` and
  `flui-types`.
- Independence holds only where the plugin-facing surface is stable for a major; the pre-1.0
  ecosystems are in lockstep whatever their manifests say.
- The kind rule of ADR-0081 §3 (only `evolving` crates set their own version) changes once, with
  evidence, and not before.
- The seam's footprint stays narrow, so independent versioning is reachable (ADR-0152).
- Release work should grow with the number of capability crates, not with a version matrix.

## Considered options

The numbers are the ones ADR-0151 section 5 and the decision below cite; they stay.

1. The contract gets its own version.
2. A narrow split by an amendment to ADR-0098.
3. The contract and capability crates ride the train.

## Decision

Chosen option: 3, because it is simple and needs neither a change to the kind rule nor a new crate
while the contract's stability is unmeasured; option 1 follows once `cargo-semver-checks` shows the
contract stable across two consecutive trains (item 3 below), and option 2 is the fallback (item 4
below).

1. **The contract versions with the train now** (option 3). `flui-platform` inherits the
   workspace version like every non-`evolving` crate and keeps depending on `flui-foundation`
   exactly, as today. In 0.2.0 nothing about its dependency changes.
2. **Capability crates ship in lockstep for the pilot.** A capability crate takes the train's
   version and is released with every FLUI release. Case A is why it cannot be otherwise while the
   contract moves with the train: a capability crate built against one train does not resolve in
   an application on the next. The cost is stated plainly: every incompatible contract release
   reaches every capability crate, which is Bevy's table.
3. **The contract moves to its own version (option 1) once `cargo-semver-checks` shows it stable
   across two consecutive trains.** The check runs in each train's release against the previous
   published train, on the contract and on the five `flui-foundation` items its signatures name
   (`Point`, `Size`, `Bounds`, `EdgeInsets`, `DataTransferId`). "Stable" is a report with no
   incompatible change. The change that moves the contract also amends the kind rule of ADR-0081
   §3 so that the contract, and then each capability crate, may set its own version, and
   reproduces cases A and B with the real crates. A capability crate versions on its own only
   after the contract does.
4. **Option 2 is the fallback, not the plan.** If a train reports an incompatible change to one of
   the five items that could not be avoided, the narrow split is proposed by an amendment to
   ADR-0098, with that report as its evidence. No geometry crate is created before that.
5. **The seam's footprint is kept narrow so that independence is reachable.** The seam names
   `WindowRef`, not `PlatformWindow`, so no window-trait method is semver surface for capability
   crates; `Plugin` and the registrar live in the contract, so a capability crate depends on one
   small surface and not on `flui-sdk` or `flui-runtime` (ADR-0152). A capability crate that names
   a `PlatformWindow` method, or a contract item outside the seam, widens the surface that
   decides how often option 1 is tested.

### Consequences

- Neutral, because 0.2.0 ships with the contract on `flui-foundation`, unchanged, and every
  capability crate at the train version.
- Neutral, because ADR-0154's capability crates depend on the contract (and, through it,
  `flui-foundation`) and ship in lockstep until the contract has its own version.
- Good, because the kind-rule change is made once, with evidence, and not before: until then
  `xtask workspace` keeps requiring the workspace version for every non-`evolving` crate.
- Bad, because release work grows with the number of capability crates, each published with every
  train, though not with a version matrix.
- Neutral, because if the check never comes clean, lockstep stays; that is workable before 1.0, as
  the ecosystems under More information show, and it is a measured outcome, not an assumption.

### Confirmation

- `cargo-semver-checks` runs in each train's release against the previous published train, on the
  contract and on the five `flui-foundation` items its signatures name (`Point`, `Size`, `Bounds`,
  `EdgeInsets`, `DataTransferId`); "stable" is a report with no incompatible change, and two
  consecutive stable trains move the contract to its own version (item 3).
- The change that moves the contract also amends the kind rule of ADR-0081 §3 and reproduces cases
  A and B with the real crates (item 3).
- Until then `xtask workspace` keeps requiring the workspace version for every non-`evolving`
  crate (see Consequences).
- The `capability` kind rule of `cargo xtask workspace` (ADR-0154 decision 1) scans a capability
  crate for `PlatformWindow` and for contract paths outside the seam, which would widen the surface
  that decides how often option 1 is tested (item 5).

## Pros and cons of the options

### 1. The contract gets its own version

- Good, because it is the narrowest change that unties the capability crates from the train: the
  contract versions on its own like `evolving` crates today, releases a compatible version on each
  train and bumps its own major or 0.N only when its own surface, or one of the five items,
  changes incompatibly (case B).
- Good, because it needs no new crate.
- Bad, because it needs a change to the kind rule that makes every non-`evolving` crate inherit the
  workspace version.
- Bad, because it needs `cargo-semver-checks` on the contract and on those five items in each
  release.
- Bad, because it is safe only once the contract is stable; while the contract moves with the
  train, a capability crate built against one train does not resolve in an application on the next
  (case A).

### 2. A narrow split by an amendment to ADR-0098

- Good, because it removes the geometry values the contract names from the train: only those
  values (today `Point`, `Size`, `Bounds`, `EdgeInsets`), f64, no units, in the smallest home that
  does not ride the train.
- Bad, because it supersedes ADR-0098 in part, and is needed only if option 1 proves insufficient.

### 3. The contract and capability crates ride the train

- Good, because it is simple: each is released with every FLUI release, and nothing about the
  contract's dependency changes in 0.2.0.
- Bad, because every incompatible contract release reaches every capability crate, which is the
  cost Bevy's ecosystem pays on every release.

## More information

- **Revisit when.** The contract moves to its own version (option 1) once `cargo-semver-checks`
  shows it stable across two consecutive trains; a capability crate versions on its own only after
  the contract does. If a train reports an incompatible change to one of the five items that
  could not be avoided, the narrow split (option 2) is proposed by an amendment to ADR-0098 with
  that report as its evidence; no geometry crate is created before that.
- **Sources.** The ecosystems read for this record, pinned as `owner/repo@sha path:line`, follow.

### What the other ecosystems show

Independence holds where the plugin-facing surface is stable for a major, and only there.
Tauri's plugins carry their own versions (geolocation 2.5.0, haptics 2.4.2) against one `tauri`
2.x because the plugin API has stayed additive through 2.0–2.12
([tauri-apps/plugins-workspace@11211c30e7 plugins/geolocation/Cargo.toml](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/Cargo.toml), [plugins/haptics/Cargo.toml](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/haptics/Cargo.toml);
[tauri-apps/tauri@9318ab84b8 crates/tauri/Cargo.toml:6](https://github.com/tauri-apps/tauri/blob/9318ab84b8/crates/tauri/Cargo.toml#L6), `links = "Tauri"`, forces one `tauri`).
Flutter's platform interface is add-only: a new method has a default body, and
`PlatformInterface.verify` rejects an implementation that uses `implements` where it should
`extend` ([flutter/packages@1383427519
packages/plugin_platform_interface/lib/plugin_platform_interface.dart:105](https://github.com/flutter/packages/blob/1383427519/packages/plugin_platform_interface/lib/plugin_platform_interface.dart#L105)). The pre-1.0
ecosystems are in lockstep whatever their manifests say: `bevy_egui` publishes a table from each
Bevy minor to its own minor ([mvlabat/bevy_egui@c6d5cdb9b1 README.md:141-154](https://github.com/mvlabat/bevy_egui/blob/c6d5cdb9b1/README.md#L141-L154)),
`dioxus-sdk-geolocation` 0.7.0 goes with Dioxus 0.7 ([DioxusLabs/sdk@551cd80082
packages/geolocation/Cargo.toml:3](https://github.com/DioxusLabs/sdk/blob/551cd80082/packages/geolocation/Cargo.toml#L3)), and Crux's capability crates are minor-locked to the core
([redbadger/crux@1ef0f547e6 crux_time/Cargo.toml:3,16](https://github.com/redbadger/crux/blob/1ef0f547e6/crux_time/Cargo.toml#L3)). Capacitor's official plugins follow the
core's major ([ionic-team/capacitor-plugins@87c0bb8045 share/package.json:3,65](https://github.com/ionic-team/capacitor-plugins/blob/87c0bb8045/share/package.json#L3)). A second reason
to version a capability crate apart is native toolchain churn: a Tauri plugin release absorbed a
new Xcode without a core release ([tauri-apps/plugins-workspace@11211c30e7
plugins/geolocation/CHANGELOG.md](https://github.com/tauri-apps/plugins-workspace/blob/11211c30e7/plugins/geolocation/CHANGELOG.md), 2.4.2).
