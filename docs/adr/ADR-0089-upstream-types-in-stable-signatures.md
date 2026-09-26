# ADR-0089: Stable signatures carry no upstream type except raw-window-handle

- **Status:** Proposed
- **Date:** 2026-09-25
- **Related:** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §1 (`ImeEvent` mirrors
  winit but is our own type), [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md) §1,
  [ADR-0037](ADR-0037-presentation-ownership-domains.md) §6 (`cursor_icon` end to end),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) (raw handles are never saved),
  [ADR-0071](ADR-0071-macos-binds-appkit-through-objc2.md),
  [ADR-0073](ADR-0073-uikit-scene-ownership.md) (raw-window-handle 0.6.2),
  [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (wire role names),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tiers and stability kinds),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) (the contract crate),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (`flui-sdk` and the facade),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) (`flui-protocol`)
- **Refs:** decision D8 and owner decision 3 in the [decision index](../../design/decisions.md);
  panel record in
  [`report-decisions.ru.md`](../research/2026-09-25-architecture-review/report-decisions.ru.md) §3

## Context

[ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) and
[ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) name three Stable crates — `flui`,
`flui-platform-api` and `flui-protocol` — whose public surface is frozen at H3. A Stable crate can
only keep a semver promise that its signatures do not delegate to someone else's release cadence:
if a public function returns `accesskit::Role`, every breaking accesskit release is a breaking
FLUI release.

Upstream types reach public paths today in four ways, each checked in this tree:

- **Input vocabulary.** The facade re-exports ui-events and keyboard-types types directly:
  `src/interaction.rs:14-16` (`PointerEvent`, `PointerButtons`, `PointerType`, `CursorIcon`) and
  `src/interaction.rs:42-45` (`Code`, `Key`, `KeyState`, `KeyboardEvent`, `Location`, `Modifiers`,
  `NamedKey`). Their source is `crates/flui-interaction/src/events.rs:93-105` and `:131-142`
  (`pub use ui_events::…`), and on the platform side `crates/flui-platform/src/traits/input.rs:60-66`
  and `crates/flui-platform/src/traits/mod.rs:60` (`pub use keyboard_types::…`). ui-events 0.3
  itself re-exports keyboard-types by glob (`ui-events-0.3.0/src/keyboard/mod.rs:21`,
  `pub use keyboard_types::*;`), so the closure of those paths is keyboard-types' whole surface.
- **Accessibility.** `flui-testing` re-exports `accesskit::{Action, ActionData, ActionRequest,
  NodeId, Point, Rect, Role, Toggled, TreeId}` (`crates/flui-testing/src/a11y.rs:36-39`), and the
  facade re-exports that module as `flui::testing::a11y` behind its `testing` feature
  (`src/lib.rs:123-124`, `src/testing.rs:9`).
  `PlatformAccessibility::publish` takes an `accesskit::TreeUpdate`
  (`crates/flui-semantics/src/platform.rs:79`, moved from `flui-platform` by ADR-0082 §2). FLUI's own vocabulary already exists:
  `SemanticsRole` has 33 variants including `None` (`crates/flui-semantics/src/role.rs:32`),
  `SemanticsAction` 24 (`crates/flui-semantics/src/action.rs:23`), and neither is
  `#[non_exhaustive]`.
- **GPU and native.** `pub use ::wgpu` (`crates/flui-engine/src/lib.rs:229`);
  `pub use android_activity` (`crates/flui-app/src/lib.rs:116`, re-exported again by the facade at
  `src/lib.rs:157`).
- **Raw handles.** `PlatformWindow` already exposes only the borrowed
  `HasWindowHandle`/`HasDisplayHandle` path (`crates/flui-platform-api/src/platform_window.rs`),
  which [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md) moved the renderer to. The
  legacy `Window` trait (`crates/flui-platform/src/window.rs:53`) still has
  `fn raw_window_handle(&self) -> RawWindowHandle` (`window.rs:196`), returning a crate-local
  enum (`window.rs:265`); nothing outside the crate uses it.

The review measured the churn this would import (crates.io, breaking releases between 2024-01 and
2026-09, recorded in the panel report): accesskit 13, wgpu 11, parley/fontique 11, ui-events 4
since 2025-05, keyboard-types 1, raw-window-handle 0 (its 0.7 milestone was closed empty), serde
and cursor-icon 0. accesskit 0.25 has 182 roles and marks one item `#[non_exhaustive]` in the
whole roles file. These counts are the panel's; this ADR does not re-derive them.

[ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) fixes the agent wire: "`role` is an
AccessKit role name in snake case", with the OS name in `native_role`. That is a string
vocabulary on the wire, not a Rust type in a signature, and this ADR keeps it.

## Decision

### 1. The rule

No item reachable from the public API of a Stable crate names a type, trait or re-export from an
upstream crate, except the ones listed in §2. "Reachable" means the transitive closure of public
signatures and public re-exports, including trait bounds and associated types, as rustdoc JSON
reports it. Evolving and internal crates are not bound by this rule; they are bound by the reach
facts of [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) instead.

The promise follows the item, not its crate. A Stable signature already names types from
internal crates (`flui-platform-api`'s window contract takes `Size<Pixels>` and `EdgeInsets`
from `flui-geometry`/`flui-types`, `PlatformWindow::resize` and `safe_area_insets` in
`crates/flui-platform-api/src/platform_window.rs`), and the
facade re-exports whole internal crates (`pub use flui_types as types`, `src/lib.rs:150`). Every
item reachable from a Stable crate's public API therefore carries the Stable promise, whatever
its crate's `tier-kind`; the `internal` kind's "no promise" covers only items outside that
closure. The closure gate of §6 is what lists them.

### 2. What may appear

| Class | Crates | Condition | Cost, recorded here |
|---|---|---|---|
| **Named exception** | `raw-window-handle` 0.6 | Only the traits `HasWindowHandle` and `HasDisplayHandle`, the borrowed `WindowHandle<'_>`/`DisplayHandle<'_>` their methods return, and the error type `HandleError`. No `RawWindowHandle`/`RawDisplayHandle` value in any Stable signature. | A raw-window-handle 0.7 is a major release of `flui-platform-api` and of `flui`. |
| **Allowed 1.x** | `serde`, `schemars`, `cursor-icon` | serde and schemars only through derive impls behind a feature of the same name (`flui-protocol`, [ADR-0095](ADR-0095-agent-protocol-schema-crate.md)); cursor-icon as `CursorIcon`. | An upstream major is our major; neither has had one in the window measured. |
| **Never** | accesskit, ui-events, keyboard-types, dpi, wgpu, kurbo, peniko, parley, fontique, cosmic-text, android-activity, and every OS binding crate ([ADR-0082](ADR-0082-platform-api-contract-crate.md)) | — | — |

The "never" list is illustrative for review; the gate in §6 is a denylist of paths plus an
allowlist, and an upstream crate not on either list fails closed.

### 3. Accessibility vocabulary is our own, not an AccessKit mirror

- `SemanticsRole`, `SemanticsAction` and the semantics flags become `#[non_exhaustive]` and move
  to `flui-protocol` ([ADR-0095](ADR-0095-agent-protocol-schema-crate.md)).
- Naming rule: use the AccessKit or ARIA name when the concept exists there. This is guidance for
  naming, not a 1:1 contract; FLUI does not add the ~150 roles nothing in the catalog produces.
- The mapping to accesskit stays internal (`flui-semantics`, and `PlatformAccessibility` lives in
  `flui_semantics::platform`, internal and tier S, re-exported at `flui_platform::traits`; never
  in `flui-platform-api`; ADR-0082 §2, amended).
- Outbound pin (FLUI → accesskit): a generated `const ALL: &[SemanticsRole]` and the same for
  actions. A test asserts that every role except `None` maps to `Some(accesskit::Role)`, and that
  every action has an inbound source or is marked FLUI-only. A test over `ALL`, not an exhaustive
  `match`, because after `#[non_exhaustive]` a match in another crate needs a `_` arm and stops
  pinning anything.
- Inbound pin (accesskit → FLUI): `semantics_action_for` and `semantics_action_args_for` match
  exhaustively, with no `_` arm (today both end in `_ => None`:
  `crates/flui-semantics/src/accesskit_translation.rs:316` and `:359`). An accesskit bump that adds
  an action then fails to compile until someone decides what it means.
- No exhaustive match over `accesskit::Role`: no production code matches on it, so a pin there
  would guard nothing.
- The [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) wire is unchanged. The wire `role`
  string is produced from the internal mapping, so it stays an AccessKit snake-case name, and a
  role with no counterpart is `unknown` plus `native_role`, as ADR-0080 already says.

### 4. Input vocabulary is our own

`flui-platform-api` defines `PointerEvent`, `KeyEvent`, `Key`, `NamedKey`, `Code`, `Modifiers`,
`ScrollDelta` and `PointerId`. `NamedKey` and `Code` are generated by a `cargo xtask` command from
the keyboard-types source, with a round-trip test over every variant and a diff check when
keyboard-types is bumped. A W3C key that keyboard-types adds before FLUI adopts it arrives as
`Key::Unidentified`; that is a deliberate, documented loss, not a silent one. No glob re-export
from ui-events reaches a Stable path.

### 5. Escape modules are Evolving and arrive with a consumer

Interop that must name an upstream type lives in a versioned module of the Evolving `flui-sdk`
([ADR-0088](ADR-0088-official-packages-sdk-and-facade.md)), named for the upstream major it pins,
and never in a Stable crate:

- Now: `flui_sdk::gpu` behind a feature named for the pinned wgpu major (`wgpu-30` for the
  workspace's `wgpu = "30.0"`, `Cargo.toml:224`). Its consumer is external GPU content. `pub use
  ::wgpu` leaves every path reachable from a Stable crate.
- Later, each only in the PR that adds its first consumer and additively in a minor release: an
  accesskit role/property hook (`flui_sdk::a11y::accesskit_NNN`) and a ui-events module. The
  accesskit hook runs last, may fill only properties the owning semantics model left empty, and
  cannot change node identity or tree structure.
- `android_activity` moves to an Evolving `native` module. The `flui-testing` accesskit
  re-exports move to an internal or Evolving path; Stable test helpers assert on `SemanticsRole`
  and `SemanticsAction`.

### 6. The gate

A `cargo xtask api-closure` command (working name) walks rustdoc JSON for `flui`,
`flui-platform-api` and `flui-protocol`, applies the denylist and the allowlist of §2, and fails
on any other upstream path. Before it joins `cargo xtask checks`, it is proven able to fail: a
scratch crate with `pub fn f() -> accesskit::Role` must be rejected (its `--self-test`, as
[ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §5 asks of every gate).

## Alternatives considered

- **Mirror AccessKit 1:1 in our own enums.** Rejected: the mirror inherits every rename and
  removal (the panel lists StaticText → Label, InlineTextBox → TextRun, ToggleButton and the
  removed deprecated roles), duplicates the 33 roles FLUI already has, and adds ~150 variants that
  nothing produces. It moves accesskit's churn into our enum instead of stopping it.
- **Re-export upstream types under versioned names in Stable crates.** Rejected: incompatible
  with an H3 freeze. egui and Masonry, which do this, make no stability promise.
- **Our own vocabulary with no escape mechanism.** Rejected: a custom widget needing a role
  outside the 33 would have no way out. The mechanism is recorded; each module waits for its
  consumer.
- **Ship the escape modules up front.** Not chosen by default: it publishes Evolving surface no
  code uses. Whether to ship them early is an open owner question
  ([open questions](../../design/open-questions.md)); the default is on demand, with
  `flui_sdk::gpu` the only one now.
- **Pin the outbound mapping with an exhaustive match.** Rejected: `#[non_exhaustive]` forces a
  `_` arm in any other crate, and the pin evaporates.

## Consequences

- Breaking changes before H3: the facade's input re-exports change type (same names, FLUI's own
  definitions); `SemanticsRole`/`SemanticsAction` gain `#[non_exhaustive]`, so downstream matches
  need a `_` arm; `flui::testing` stops exposing accesskit types; `flui::android_activity` moves;
  the legacy `Window::raw_window_handle` is deleted with the rest of the legacy `Window` family
  ([ADR-0082](ADR-0082-platform-api-contract-crate.md) §5).
- An accesskit or ui-events major becomes an internal change plus, at most, a regenerated key
  table — no FLUI major.
- The accessibility vocabulary is narrower than AccessKit's by design. A role FLUI lacks reaches
  the OS only through the Evolving hook once it exists.
- The gate needs rustdoc JSON, which may need a pinned nightly toolchain. That is a hypothesis;
  if true, one nightly pin serves this gate, the `flui-sdk` surface measurement and the H3
  semver checks together.
- Migration order: the `#[non_exhaustive]` + `ALL` test and the exhaustive inbound matches first;
  the generated input types when `flui-platform-api` is extracted
  ([ADR-0082](ADR-0082-platform-api-contract-crate.md)); moving `pub use ::wgpu` and
  `android_activity` with the `flui-sdk` work; `api-closure` in `checks` before H3.

## Verification

None of these exist yet.

- `api-closure` with its self-test probe (§6), in `cargo xtask checks`.
- The outbound `ALL` test and the exhaustive inbound matches in `flui-semantics` (§3).
- The generated-key round-trip test and the keyboard-types diff check (§4).
- A `compile_fail` or closure-gate case showing `RawWindowHandle` cannot appear in a Stable
  signature (§2).
