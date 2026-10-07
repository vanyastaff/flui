# ADR-0158: UI runtime and host vocabulary

- **Status:** Accepted
- **Date:** 2026-10-07
- **Supersedes:** ADR-0027's `UiRealm`/`RealmId` terminology; its ownership,
  threading, presentation and shutdown decisions remain in force.

## Context

The independent UI owner contains mutable UI state, scheduling, a command inbox
and the frame transaction for one or more presentations. The former name
`UiRealm` describes isolation but does not explain what the object executes.
Calling it a host would obscure the distinction from the application and
headless environments that supply its execution and presentation facilities.

There is no common name for this complete boundary across UI frameworks.
Compose separates [Composition and Recomposer](https://developer.android.com/reference/kotlin/androidx/compose/runtime/Recomposer);
its multiplatform UI container is an internal
[ComposeScene](https://github.com/JetBrains/compose-multiplatform-core/blob/jb-main/compose/ui/ui/src/skikoMain/kotlin/androidx/compose/ui/scene/ComposeScene.skiko.kt).
SwiftUI exposes [Scene](https://developer.apple.com/documentation/swiftui/scenes)
and a [UIHostingController](https://developer.apple.com/documentation/swiftui/uihostingcontroller)
for UIKit integration. Masonry's
[RenderRoot](https://docs.rs/masonry/latest/masonry/app/struct.RenderRoot.html)
owns its widget tree. These are comparisons of responsibilities, not equivalent
types or an industry naming standard.

## Decision

Name the independent UI state and execution owner `UiRuntime`, its generational
identity `UiRuntimeId`, and the identity field in `PresentationAddress`
`ui_runtime_id`. Its module is `ui_runtime`. Use host vocabulary for the
environment that supplies facilities to runtimes, and presentation vocabulary
for the independently identified representations of a runtime.

One host may serve several independent UI runtimes. One UI runtime may have
several presentations. A runtime is not a native window or an owner thread.
This naming change does not merge their lifetimes, share their schedulers or
change the generation checks established by ADR-0027 and ADR-0037.

`UiSession` was considered because a runtime has an independent lifetime, but
it underemphasizes execution and can suggest a user/document session or restored
session metadata. `UiHost` was considered because it owns UI, but conflates the
hosted state with its hosting environment. `UiRuntime` states the object's
responsibility while preserving the host/presentation distinction.

## Consequences

This is a breaking vocabulary migration, with no compatibility aliases retaining
the former names. Production consumers, test fixtures, current documentation and
tooling use the new identifiers. Existing ADR numbers and historical filenames
remain stable; archived plans and research retain their original evidence.

The owner-delivery extraction of ADR-0083 is a separate responsibility change.
Renaming its participants does not complete that extraction or close its frame
driver and operation migration.
