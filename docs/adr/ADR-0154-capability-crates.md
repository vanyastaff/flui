# ADR-0154: Capability crates carry their own OS code

- **Status:** Proposed (2026-10-06). Awaiting the owner's approval; nothing in this ADR is
  implemented. The workspace rule lands with the first capability crate.
- **Date:** 2026-10-06
- **Amends, on acceptance:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §3 (a new
  kind, `capability`) and §2 (its reach table);
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (official packages keep their rules;
  capability crates are a second kind of package with different ones).
- **Depends on:** [ADR-0153](ADR-0153-stable-crates-do-not-ride-the-train.md) (the contract is
  off the train), [ADR-0152](ADR-0152-capability-seam-revised.md) (the seam).
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §3 (the core host's
  closed list)

## Context

Toward 1.0, FLUI needs the services applications expect from a platform: location, sensors,
camera and media, notifications, secure storage and biometrics, device state, sharing, file
dialogs, launchers, tray and menus. Each needs OS code. Today OS code may live only in the core
host (`allowed-dependents = ["flui-app"]`), and the `pkg` reach table forbids `windows-*`,
`objc2-*`, `jni-*`, `ndk-*` and `android-*`. Putting every service in the core host would make
it a monolith released with the train, put every service's trait in the Stable contract, and
leave third parties no way to add one.

Other frameworks agree: the core owns what every application needs; an optional service is its
own package with its own OS code and version (Flutter federated plugins, Tauri plugins, Bevy
plugins, Slint backends). Flutter's three-package federation exists because Dart has no
conditional compilation; in Rust one crate with `cfg`-gated backends does the same job, and a
replacement implementation is a provider override through the seam.

## Decision

1. **A new kind, `tier-kind = "capability"`,** in tier `pkg`. A capability crate:
   - depends normally only on the contract crate, `flui-geometry`, optionally `flui-sdk` (only
     if it ships widgets), and external crates;
   - may reach OS crates: its reach table is the `pkg` table without the OS-crate globs, and it
     still forbids `winit`, `wgpu`, `tokio`, the core host, `flui-app` and `flui-engine`;
   - is a **leaf**: no FLUI crate and no other capability crate depends on it normally;
   - is versioned on its own (`version` set in its manifest, not inherited from the train);
   - registers through the seam (ADR-0152) and ships a headless or simulated provider and a
     conformance table that every backend it claims must pass.
2. **One crate per permission set and release cadence, not per API.** Services that share an OS
   permission and are used together share a crate (for example accelerometer, gyroscope,
   magnetometer and barometer). Each new capability crate is introduced by its own ADR naming
   its permission set and its claimed platforms.
3. **Shared code goes down, not sideways.** What several capability crates need (reaching the
   Android activity and its results, application-delegate events, window-message hooks) is a
   host bridge in the contract and the core host, decided by its own ADR with the first crate
   that needs it. There is no shared "capability common" crate; small OS glue is duplicated
   rather than centralised.
4. **Claimed support is executed support.** A capability crate's platform table lists a
   platform as supported only when its conformance table runs on that platform in CI or in a
   recorded local run; otherwise "type-checked".
5. **The rule lands with its first user.** `cargo xtask workspace` gains the kind rules (allowed
   dependencies, leaf, own version) and `cargo xtask reach` the table, each with a `--self-test`
   case that plants a violation, in the pull request that adds the first capability crate.

## Alternatives considered

| Alternative | Why not |
|---|---|
| All OS code in the core host | A monolith released with the train; every service's trait in the Stable contract; no third-party services |
| One `flui-services` crate with a feature per service | One breaking change in one service breaks every user; feature combinations multiply; the manifest carries every OS dependency |
| Flutter-style federation (interface, implementations, app-facing package) | Rust's `cfg` makes it unnecessary; triples the crate count |
| Allow OS crates in official packages | Official packages are UI libraries on the SDK; mixing OS code into them couples UI releases to OS work |

## Consequences

- The core host stays bounded by ADR-0151 §3's closed list.
- An application compiles and declares permissions only for the services it uses.
- Release work grows with the number of capability crates; changelog fragments and automated
  version bumps keep it per crate.
- Capability crates are not part of the facade; they are found through the documentation and the
  command-line tool.
