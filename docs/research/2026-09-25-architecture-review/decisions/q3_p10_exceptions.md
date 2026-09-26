# Decision panel: q3_p10_exceptions

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Workspace pins (D:\\flui\\Cargo.toml): accesskit = \"0.25\" (:144), accesskit_consumer 0.39 (:147), wgpu 30.0 (:224), winit 0.30.12 (:232), raw-window-handle 0.6 (:233), ui-events 0.3 (:301), cursor-icon 1.2 (:302). Cargo.lock resolves accesskit 0.25.0, ui-events 0.3.0, keyboard-types 0.8.3, raw-window-handle 0.6.2, kurbo 0.13.1, dpi 0.1.2, wgpu 30.0.1, cursor-icon 1.2.0. peniko and parley are not in the lockfile yet: text is cosmic-text 0.19 (crates/flui-painting/Cargo.toml:33), and Parley is only planned (D12). kurbo is an optional feature bridge in flui-geometry (Cargo.toml:37,52).",
    "Upstream re-exports in production code today (from `grep -rnE 'pub use (accesskit|ui_events|wgpu|raw_window_handle|kurbo|keyboard_types|cursor_icon|winit|android_activity)'`): crates/flui-semantics/src/lib.rs:103 `pub use accesskit::{NodeId, TreeUpdate}`; crates/flui-testing/src/a11y.rs:36 `pub use accesskit::{Action, ActionData, ActionRequest, NodeId, Point as A11yPoint, Rect as A11yRect, Role, Toggled, TreeId}`; crates/flui-interaction/src/events.rs:93,102,142 plus ids.rs:67 (ui_events pointer/keyboard/ScrollDelta/PointerId); crates/flui-platform/src/traits/input.rs:60-66 (keyboard_types::{Key,Modifiers}, ui_events KeyboardEvent/pointer/ScrollDelta); crates/flui-platform/src/traits/mod.rs:60 keyboard_types::NamedKey; crates/flui-platform/src/lib.rs:191 and flui-interaction/src/events.rs:129 cursor_icon::CursorIcon; crates/flui-engine/src/lib.rs:229 `pub use ::wgpu;`; crates/flui-app/src/lib.rs:116 `pub use android_activity;`; crates/flui-geometry/src/bridges/mod.rs:12 kurbo::KurboBridgeError.",
    "Transitive glob: ~/.cargo/registry/src/*/ui-events-0.3.0/src/keyboard/mod.rs:21 contains `pub use keyboard_types::*;`. So FLUI's re-export of ui_events::keyboard also brings keyboard-types 0.8 into the public contract without anyone deciding to. ui-events 0.3 depends on dpi ^0.1.2, keyboard-types ^0.8.0 and optionally kurbo ^0.13 (crates.io dependencies API).",
    "The accesskit types sit on the platform seam: crates/flui-platform/src/traits/accessibility.rs:34 `use accesskit::{ActionRequest, TreeUpdate};`, `PlatformAccessibility::publish(&self, update: TreeUpdate)`, and `AccessibilityActionListener = Arc<dyn Fn(ActionRequest) + Send + Sync>`. The module doc (:11-17) says the seam intentionally 'speaks AccessKit, never semantics types', because flui-platform (layer 2) cannot name flui-semantics (layer 3). The headless backend exposes `pub fn published() -> Vec<accesskit::TreeUpdate>` and `pub fn request_action(accesskit::ActionRequest)` (crates/flui-platform/src/platforms/headless/platform.rs:1371,1412).",
    "FLUI already owns a Flutter-shaped vocabulary: `pub enum SemanticsRole` (crates/flui-semantics/src/role.rs:32, 33 variants, #[repr(u32)], NOT #[non_exhaustive]) and `pub enum SemanticsAction` (crates/flui-semantics/src/action.rs:23, about 24 variants, NOT #[non_exhaustive]). Interactive kinds are flags rather than roles (role.rs:26-29). crates/flui-semantics/src/accesskit_translation.rs already maps flags plus role to accesskit::Role (e.g. :101-118 Switch/RadioButton/CheckBox/Button/Link/MultilineTextInput), and back with `semantics_action_for(accesskit::Action) -> Option<SemanticsAction>` (:299) and `tree_to_update` (:553).",
    "The accesskit 0.25.0 source has 182 Role variants (`awk '/pub enum Role \\{/,/^\\}/' src/lib.rs | grep -cE '^\\s+[A-Z][A-Za-z]*,'`) and 22 Action variants. The crate has only 1 `#[non_exhaustive]` in the whole file, and neither Role nor Action carries it. So each Role or Action that accesskit adds is a semver-major change for anyone who matches on its enums. With 33 owned roles against 182 AccessKit roles, the owned vocabulary cannot map 1:1.",
    "The accesskit CHANGELOG (0.25.0 registry copy) lists breaking changes that hit Role, Action and TreeUpdate directly: 'Rename Checked to Toggled; drop ToggleButton role' (#388), 'Rename the StaticText role to Label' (#434), 'Rename Role::InlineTextBox to TextRun' (#473), 'Drop DefaultActionVerb' (#472), 'Refactor actions for scrolling by discrete units' (#573), 'ScrollIntoView' refactor (#594), 'Drop deprecated roles' (#642), 'Multiple tree support' (#655, TreeUpdate/TreeId shape), and 'Rename NodeBuilder to Node' (#476).",
    "raw-window-handle in public signatures: crates/flui-platform/src/window.rs:196 `fn raw_window_handle(&self) -> RawWindowHandle;`, crates/flui-engine/src/window_target.rs:13 (HasWindowHandle/HasDisplayHandle bounds), and crates/flui-engine/src/error.rs:164 (`source: raw_window_handle::HandleError`, which is itself #[non_exhaustive], handled at :249-262). wgpu 30.0.1's Cargo.toml depends on raw-window-handle (registry wgpu-30.0.1/Cargo.toml:186).",
    "Public items naming wgpu:: are confined to flui-engine: texture_pool.rs (5), renderer.rs (4), painter/mod.rs (3), offscreen/mod.rs (1), plus the `pub use ::wgpu` at lib.rs:229. There are 2 `pub .*cosmic_text::` hits. The facade src/ has no pub re-export of any of these crates (grep of src/*.rs).",
    "Architecture doc (scratchpad flui-global-architecture.md): P10 at :90 is phrased by major cadence with named exceptions. Tier C at :102 already lists 'serde, raw-window-handle as exception', so serde is a second implicit exception and raw-window-handle is not the only one. :292 decides that flui-protocol defines its own Role/Action 'with names matching AccessKit' and a tested 1:1 mapping. :314 decides that wgpu interop lives permanently in Evolving `flui_sdk::gpu`, which re-pins the wgpu major on each release train."
  ],
  "market_precedents": [
    {
      "who": "AccessKit (accesskit crate)",
      "what": "13 semver-breaking releases from 2024-01 to 2026-09: 0.13 through 0.25, most recently 0.25.0 on 2026-08-29. That is about one breaking release every 2.5 months. accesskit_winit had 17 in the same window. No #[non_exhaustive] on Role or Action.",
      "outcome_or_lesson": "Any Stable crate that names accesskit types gets a forced major every few months. Even an owned Role/Action enum mirroring AccessKit 1:1 would churn, because roles themselves get renamed or dropped (StaticText->Label, InlineTextBox->TextRun, ToggleButton dropped, deprecated roles dropped). A 1:1 mirror moves the churn but does not remove it.",
      "source": "crates.io API https://crates.io/api/v1/crates/accesskit/versions (script semver_breaks.py in scratchpad); accesskit-0.25.0/CHANGELOG.md"
    },
    {
      "who": "ui-events (linebender)",
      "what": "4 breaking releases between 2025-05 and 2026-01 (0.0.1, 0.1, 0.2, 0.3). The keyboard module does `pub use keyboard_types::*`.",
      "outcome_or_lesson": "Young and fast-moving, and it drags keyboard-types (0.8, one break since 2024) and dpi (0.1) into the public contract. Not fit for a Stable signature. Wrap it in owned pointer/key types; keyboard-types' W3C-shaped Key/Code are a good model to copy the names from.",
      "source": "https://crates.io/api/v1/crates/ui-events/versions; ui-events-0.3.0/src/keyboard/mod.rs:21"
    },
    {
      "who": "wgpu",
      "what": "11 breaking releases from 2024-01 to 2026-09 (0.19, 0.20, then 22 through 30), roughly quarterly. 30.0.0 shipped on 2026-07-01.",
      "outcome_or_lesson": "Leaving 0.x did not slow the cadence (P10 is correctly phrased by major cadence, not by 0.x). Keep wgpu out of Stable; confirms the doc's `flui_sdk::gpu` Evolving plan and the removal of `pub use ::wgpu` from flui-engine.",
      "source": "https://crates.io/api/v1/crates/wgpu/versions"
    },
    {
      "who": "raw-window-handle (rust-windowing)",
      "what": "0 breaking releases since 0.6.0 (2023-10); latest is 0.6.2 (2024-05-17). The GitHub v0.7 milestone is closed with 0 issues (closed 2026-02-10). wgpu 30, winit 0.30 and bevy_window 0.19 all depend on ^0.6.",
      "outcome_or_lesson": "The only pre-1.0 crate here that is de-facto frozen and is the ecosystem's interop currency. A justified named exception, provided it is exposed only through HasWindowHandle/HasDisplayHandle (and HandleError), not through re-exported enums.",
      "source": "https://crates.io/api/v1/crates/raw-window-handle/versions; https://github.com/rust-windowing/raw-window-handle/milestone/1"
    },
    {
      "who": "kurbo / peniko / parley / fontique / cosmic-text",
      "what": "Breaking releases 2024-01 to 2026-09: kurbo 3, peniko 6, parley 11, fontique 11, cosmic-text 9.",
      "outcome_or_lesson": "None qualify for Stable. Geometry and color already have owned types (flui-geometry, flui-types), and the kurbo bridge is a feature. Once Parley lands (D12), its Layout/FontContext types must stay behind an owned shaped-run contract and never appear in Stable signatures.",
      "source": "https://crates.io/api/v1/crates/{kurbo,peniko,parley,fontique,cosmic-text}/versions"
    },
    {
      "who": "serde / cursor-icon",
      "what": "serde 1.x: 0 breaking releases (1.0.229 in 2026-07). cursor-icon 1.x: 0 breaking releases since 1.0 (latest 1.2.0, 2025-05).",
      "outcome_or_lesson": "Both already satisfy the cadence rule, so under the rephrased P10 they are allowed rather than exceptions. The tier ADR should list them as 'allowed 1.x upstream' so the cargo-public-api closure check does not flag them. cursor-icon is currently re-exported by flui-platform and flui-interaction.",
      "source": "https://crates.io/api/v1/crates/serde/versions; crates.io cursor-icon versions; crates/flui-platform/src/lib.rs:191"
    },
    {
      "who": "egui 0.36",
      "what": "`pub use accesskit;` at the crate root, plus epaint/ecolor/emath. The docs state 'each new version having breaking changes'.",
      "outcome_or_lesson": "Re-exporting accesskit works only because egui itself makes no stability promise. Not a model for a Stable tier.",
      "source": "https://docs.rs/egui/latest/egui/ ; crates.io egui 0.36.2 deps accesskit ^0.24.1"
    },
    {
      "who": "Masonry / Xilem 0.4",
      "what": "masonry_core re-exports accesskit, dpi, parley, ui_events, vello (with kurbo/peniko) at the root. Its deps are pinned to accesskit ^0.21, parley ^0.6, ui-events ^0.2, several majors behind current.",
      "outcome_or_lesson": "Wholesale re-export ties releases to the Linebender train and leaves the crate lagging upstream. That is fine for a pre-1.0 ecosystem that releases in lockstep, and a cautionary tale for a framework promising Stable.",
      "source": "https://docs.rs/masonry_core/latest/masonry_core/ ; crates.io masonry_core 0.4.0 dependencies"
    },
    {
      "who": "Bevy 0.19",
      "what": "Since 0.15, bevy_a11y no longer re-exports accesskit (the docs say so explicitly). Yet `AccessibilityNode` still wraps a public `accesskit::Node`, and bevy_window depends on raw-window-handle ^0.6.",
      "outcome_or_lesson": "Removing the re-export without removing the type from signatures only forces users to add a version-matched accesskit dependency. This is the half-measure FLUI's flui-semantics/lib.rs:97-103 comment argues against. The real fix is keeping accesskit out of the signature, not just out of `pub use`.",
      "source": "https://docs.rs/bevy_a11y/latest/bevy_a11y/"
    },
    {
      "who": "iced 0.14",
      "what": "iced_core defines its own keyboard, mouse and touch modules, Point/Size/Rectangle/Color. Its only third-party re-export is SmolStr. winit lives in iced_winit.",
      "outcome_or_lesson": "The owned-vocabulary precedent for input and geometry: the core crate is insulated from winit, keyboard-types and ui-events churn. Matches wrapping ui-events and keyboard-types in flui-platform-api.",
      "source": "https://docs.rs/iced_core/latest/iced_core/"
    },
    {
      "who": "Slint 1.x (stable 1.0 since 2023)",
      "what": "Upstream types are exposed only behind version-suffixed features: `raw-window-handle-06` (no unstable prefix), versus `unstable-wgpu-29`, `unstable-wgpu-30`, `unstable-winit-030` and `unstable-fontique-011`.",
      "outcome_or_lesson": "The closest precedent for FLUI's P10: exactly one upstream crate, raw-window-handle, is treated as stable-exposable, and wgpu, winit and fontique are explicitly unstable and version-suffixed. Suggests versioned module or feature names (e.g. `flui_sdk::gpu` behind a `wgpu-30` feature) so that one wgpu major is not a breaking change for FLUI. Slint's own AccessibleRole enum mapped to accesskit internally was not verified from the docs fetch (hypothesis).",
      "source": "https://docs.rs/crate/slint/latest/features"
    }
  ],
  "constraints": [
    "Confirmed: raw-window-handle 0.6 is the only upstream crate that fails the '1.x' test yet passes the cadence test (0 breaks in about 3 years; v0.7 milestone closed empty). It is the correct sole NAMED exception, but only through the trait bounds HasWindowHandle/HasDisplayHandle and the #[non_exhaustive] HandleError. `fn raw_window_handle(&self) -> RawWindowHandle` (flui-platform/src/window.rs:196) exposes the exhaustive-by-convention RawWindowHandle enum; prefer returning `WindowHandle<'_>` via `HasWindowHandle`. The tier ADR must state the price: 'rwh 0.7 means a flui-platform-api major'.",
    "serde and cursor-icon are 1.x crates that pass the cadence rule. They need an 'allowed' list in the tier ADR, not exception status. The architecture doc's C tier (:102) mentions serde alongside rwh, and the ADR should state this distinction explicitly.",
    "Correction to decision :292 (own Role/Action 'with names matching AccessKit, tested 1:1'): a 1:1 mirror of 182 AccessKit roles would inherit AccessKit's renames and drops (StaticText->Label, InlineTextBox->TextRun, ToggleButton dropped, deprecated roles dropped) and would still need breaking changes. Better: the Stable vocabulary is FLUI's existing owned SemanticsRole (33) plus flags plus SemanticsAction (about 24), made #[non_exhaustive] and moved or duplicated into flui-protocol. The AccessKit mapping lives in the internal translation module (flui-semantics/accesskit_translation.rs), with an exhaustive-match test over accesskit::Role and Action that fails to compile when accesskit adds a variant, pinning the mapping. 'Names follow AccessKit where a concept exists' is fine as a naming guideline, not as a 1:1 contract.",
    "The platform accessibility seam (PlatformAccessibility::publish(TreeUpdate), ActionRequest listeners) must stay in internal flui-platform (as :292 decides) or be re-expressed with owned types. Today it is the only reason flui-platform-api would need accesskit. A plugin-facing Stable seam cannot carry TreeUpdate, which changed shape in the #655 multi-tree support.",
    "The flui-semantics:103 `pub use accesskit::{NodeId, TreeUpdate}` exists because SemanticsUpdateCallback names TreeUpdate. If flui-semantics stays internal that is fine; any Stable path reaching SemanticsUpdateCallback must switch to an owned update type. The flui-testing a11y re-exports (a11y.rs:36: Role, Action, ActionRequest, TreeId, ...) must be Evolving or internal, or tests written against them break with every accesskit release, about 5 per year.",
    "ui-events (4 breaks in 9 months) and keyboard-types via its glob re-export (ui-events/src/keyboard/mod.rs:21) must be wrapped: owned PointerEvent/KeyEvent/Key/Code/Modifiers/ScrollDelta/PointerId in flui-platform-api, with From impls in the internal crate. keyboard-types is W3C-aligned and rarely breaks (1 break since 2024), so copying its Key/NamedKey/Code names is low-risk, but the types themselves should not be exposed.",
    "wgpu (about 4 majors a year) must not reach Stable. Remove `pub use ::wgpu` (flui-engine/src/lib.rs:229) and keep interop in Evolving `flui_sdk::gpu`, ideally version-suffixed (Slint's `unstable-wgpu-30` pattern), so that one wgpu major is not a FLUI major.",
    "kurbo, peniko, parley, fontique and cosmic-text (3 to 11 breaks since 2024) must not appear in Stable or Evolving-sdk signatures. flui-geometry/flui-types keep owned types, and the kurbo bridge stays an off-by-default feature of an internal crate. When Parley lands (D12), its types stay behind the owned shaped-run contract.",
    "android_activity is re-exported from flui-app (lib.rs:116) and appears on the doc's П15 list. It is not a candidate exception; it belongs in a platform-specific Evolving `native` module.",
    "Enforcement hypothesis (not run): a cargo-public-api snapshot of flui, flui-platform-api and flui-protocol, grepped for `accesskit::|ui_events::|keyboard_types::|wgpu::|kurbo::|peniko::|parley::|cosmic_text::`, with an allowlist of raw_window_handle, serde and cursor_icon, would mechanically enforce P10. I did not verify that cargo-public-api's output includes the transitive closure."
  ],
  "experiments_run": [
    "crates.io version-history fetch plus a Python script (scratchpad semver_breaks.py) counting semver-breaking releases from 2024-01 to 2026-09 (first release of each new major or 0.minor). Results: accesskit 13, accesskit_winit 17, ui-events 4 (since 2025-05), wgpu 11, raw-window-handle 0, kurbo 3, peniko 6, parley 11, fontique 11, cosmic-text 9, keyboard-types 1, cursor-icon 0, winit 1, dpi 2, serde 0.",
    "`grep -rnE 'pub use (accesskit|ui_events|wgpu|raw_window_handle|kurbo|keyboard_types|cursor_icon|winit|android_activity)' crates src --include=*.rs` listed 16 re-export sites (see code_facts). Per-file counts of `pub .*<crate>::` show wgpu confined to flui-engine, accesskit in flui-platform headless, flui-semantics and flui-testing, and ui_events in flui-interaction and flui-platform.",
    "Read the accesskit 0.25.0 registry source: 182 Role and 22 Action variants, 1 #[non_exhaustive] in the whole file, none on Role or Action. Extracted all 'BREAKING CHANGES' bullets from its CHANGELOG, which showed repeated Role and Action renames and removals.",
    "Counted FLUI's owned enums: SemanticsRole has 33 variants, SemanticsAction about 24, neither #[non_exhaustive] (grep returned nothing).",
    "Checked peer-framework dependencies through the crates.io dependencies API: egui 0.36.2 accesskit ^0.24.1; masonry_core 0.4.0 accesskit ^0.21.1, parley ^0.6, ui-events ^0.2, dpi, cursor-icon; bevy_a11y 0.19.1 accesskit ^0.24; bevy_window raw-window-handle ^0.6; iced_core 0.14 none of them.",
    "docs.rs WebFetch: egui re-exports accesskit; masonry_core re-exports accesskit, dpi, parley, ui_events and vello (with kurbo and peniko); bevy_a11y stopped re-exporting accesskit in 0.15 but AccessibilityNode wraps accesskit::Node; iced_core owns its keyboard, mouse, touch, geometry and Color types; Slint features include raw-window-handle-06, unstable-wgpu-29, unstable-wgpu-30, unstable-winit-030 and unstable-fontique-011. The Slint platform-module fetch could not confirm its accessibility API shape.",
    "Not run: no cargo build or cargo-public-api snapshot (read-only, and a full workspace build was disallowed). The .gpui and .flutter reference clones are absent. The cratesio MCP server failed to connect, so the crates.io REST API was queried directly with curl."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "As the doc decides: owned Role/Action mirroring AccessKit 1:1, wrap everything else",
      "description": "Follow flui-global-architecture.md:292 literally. flui-protocol defines Role/Action whose names and variant sets match AccessKit (182 roles, 22 actions in 0.25.0), with a tested 1:1 mapping. ui-events, keyboard-types, wgpu, kurbo/peniko/parley stay out of Stable. raw-window-handle is the named exception, and serde is mentioned next to it as a second exception (:102).",
      "pros": [
        "Consumers who know AccessKit or ARIA recognise every role",
        "The mapping is trivially correct because it is an identity by name",
        "The Stable crate compiles without accesskit"
      ],
      "cons": [
        "A mirror inherits AccessKit's churn, it does not remove it: StaticText->Label (#434), InlineTextBox->TextRun (#473), ToggleButton dropped (#388) and deprecated roles dropped (#642) would each force a breaking change to the mirrored enum, or leave it drifting away from 'matching names'. AccessKit had 13 breaking releases from 2024-01 to 2026-09",
        "182 roles sit next to FLUI's own 33-variant SemanticsRole (crates/flui-semantics/src/role.rs:32), which treats interactive kinds as flags (role.rs:26-29). That leaves two vocabularies and a lossy mapping between them anyway",
        "Stable surface grows by roughly 180 variants that no FLUI widget produces, which conflicts with P9's rule that unwired pub surface is removed",
        "Treats serde as an exception although it is 1.x with 0 breaking releases, which muddles the ADR"
      ],
      "cost_now": "Medium to high: writing out about 200 variants plus the mapping and tests, and reconciling them with SemanticsRole plus flags.",
      "cost_later": "High: every AccessKit role rename or removal becomes a FLUI Stable decision, either a major bump or a divergence from the 'names match' promise. Expect several such decisions a year.",
      "reversibility": "Poor after the Stable freeze (H3). Removing variants from a frozen enum is a major change, so an over-wide mirror is hard to shrink.",
      "fits_plan": "Matches the wording of :292. Contradicts the spirit of P10 and P9, since churn is relocated rather than bounded, and it duplicates the existing flui-semantics vocabulary."
    },
    {
      "id": "B",
      "name": "Strict owned vocabulary: FLUI's own semantics and input types in Stable; upstream only through a named allowlist",
      "description": "The Stable vocabulary is FLUI's existing SemanticsRole (33 variants) plus flags plus SemanticsAction (about 24), all made #[non_exhaustive] and moved into flui-protocol. The AccessKit mapping stays in the internal accesskit_translation.rs (:101-118, :299, :553) and is pinned by an exhaustive match over accesskit::Role and accesskit::Action with no wildcard arm, so a new AccessKit variant stops compilation instead of being silently dropped. The naming guideline is 'use the AccessKit or ARIA name where the concept exists', not 1:1. PlatformAccessibility::publish(TreeUpdate) and the ActionRequest listener stay in internal flui-platform. flui-platform-api owns PointerEvent, KeyEvent, Key, NamedKey, Code, Modifiers, ScrollDelta and PointerId, with names copied from keyboard-types (W3C), and From impls from ui-events in the internal crate. raw-window-handle 0.6 is the only named exception, and only through HasWindowHandle/HasDisplayHandle bounds and HandleError. `fn raw_window_handle(&self) -> RawWindowHandle` (flui-platform/src/window.rs:196) is replaced by a HasWindowHandle bound. serde 1.x and cursor-icon 1.x go on an 'allowed 1.x upstream' list, not the exception list. wgpu, kurbo, peniko, parley, fontique and cosmic-text never appear in Stable. There is no raw escape hatch.",
      "pros": [
        "Bounds churn at the source. accesskit (13 breaking releases), ui-events (4 in 9 months), wgpu (11) and parley and fontique (11 each) can no longer force a FLUI Stable major",
        "Reuses the vocabulary FLUI already owns and maps, so it adds no second enum",
        "The exhaustive-match pin turns every accesskit bump into a compile error in one internal file, which is visible and cheap to fix",
        "Keyboard-types' W3C names are stable (1 breaking release since 2024), so copied names carry little risk",
        "Matches iced_core's owned keyboard, mouse and touch modules (iced 0.14) and Slint's single stable-exposable upstream, raw-window-handle-06"
      ],
      "cons": [
        "No way to express AccessKit roles beyond the 33, such as Grid, TreeItem or Math, until FLUI adds them. Custom-widget authors are blocked in the meantime",
        "Copying NamedKey and Code is a large mechanical job. keyboard-types has hundreds of named keys (size not counted, hypothesis), which calls for generation or a curated subset",
        "Advanced GPU users lose `pub use ::wgpu` (flui-engine/src/lib.rs:229) with nothing to replace it in Stable",
        "SemanticsRole is #[repr(u32)] and exhaustive today, so adding #[non_exhaustive] breaks internal matches once. That is cheap pre-1.0"
      ],
      "cost_now": "Medium: add #[non_exhaustive] to 2 enums, move them into flui-protocol, write one exhaustive pin test, write about 8 owned input types plus conversions, change the window.rs:196 signature, remove 16 re-export sites or demote them to internal crates, and draft the tier ADR text (allowlist plus exception with its price).",
      "cost_later": "Low: an accesskit, ui-events or wgpu bump is an internal patch. New roles and actions are additive minors thanks to #[non_exhaustive]. The only forced FLUI major comes from raw-window-handle 0.7, and its milestone closed empty on 2026-02-10.",
      "reversibility": "Good. Owned #[non_exhaustive] enums can grow in minors, and an escape hatch can be added later without breaking anything. Going the other way, from re-exports to owned types, would be a major change.",
      "fits_plan": "Fits P10 as rephrased (by major cadence with named exceptions) and P9 (no unwired surface). Fits the :314 decision to keep wgpu in Evolving. Requires amending :292 (owned vocabulary instead of a 1:1 mirror) and :102 (serde is an allowed 1.x crate, not an exception)."
    },
    {
      "id": "C",
      "name": "Re-export upstream behind version-suffixed features (egui/Masonry with Slint-style naming)",
      "description": "Keep today's shape, where flui-semantics:103, flui-testing a11y.rs:36, flui-interaction and flui-platform re-export accesskit, ui_events, keyboard_types and cursor_icon, and flui-engine re-exports wgpu. Wrap each re-export in a feature or module named for its version (accesskit-025, ui-events-03, wgpu-30), so a new upstream major adds a new module instead of changing the old one.",
      "pros": [
        "Almost zero work now: the re-exports already exist",
        "The full AccessKit and ui-events expressiveness reaches users with no mapping loss",
        "Matches Masonry and egui, which are known-working ecosystems"
      ],
      "cons": [
        "Version-suffixed features only work for leaf interop modules such as GPU. For event and semantics types that run through the core pipeline, FLUI would have to maintain several upstream majors side by side, so in practice the old feature gets dropped and that is a major change",
        "ui-events' `pub use keyboard_types::*` (ui-events-0.3.0/src/keyboard/mod.rs:21) quietly pulls keyboard-types and dpi into the contract",
        "egui states that each release is breaking, and Masonry lags (accesskit ^0.21, ui-events ^0.2). Neither makes a Stable promise, so neither is a precedent for a Stable tier",
        "Bevy shows the trap: after removing the accesskit re-export, AccessibilityNode still wraps accesskit::Node, and users are left matching versions by hand"
      ],
      "cost_now": "Very low.",
      "cost_later": "Very high: about 5 accesskit, 4 wgpu and several ui-events breaking releases a year each become a FLUI Stable major or a maintained parallel feature. The H3 freeze of the Stable closure becomes impossible in practice.",
      "reversibility": "Bad. Once users depend on re-exported upstream types, replacing them with owned types is a breaking migration for every consumer.",
      "fits_plan": "Contradicts P10 outright and the H3 exit criterion (freeze the measured closure, cargo-semver-checks gating). Acceptable only for Evolving or internal crates."
    },
    {
      "id": "D",
      "name": "Hybrid: B for Stable, plus version-suffixed raw escape hatches in Evolving flui_sdk",
      "description": "Everything in option B, plus explicitly Evolving escape modules that are named for their version and carry no promise across upstream majors. `flui_sdk::gpu` sits behind a `wgpu-30` feature, as :314 already decides, using Slint's unstable-wgpu-30 naming. `flui_sdk::a11y::accesskit_025` adds a per-node extension hook, for example a custom-role or raw-properties callback applied during translation, for roles FLUI has not yet adopted. `flui_sdk::input::ui_events_03` provides raw From/Into conversions for embedders. flui-testing's accesskit re-exports (a11y.rs:36) move under the same Evolving a11y module, so Stable test helpers assert on the owned SemanticsRole and SemanticsAction. android_activity (flui-app/src/lib.rs:116) moves to an Evolving platform-specific `native` module. A cargo-public-api closure check over flui, flui-platform-api and flui-protocol fails on the patterns accesskit::, ui_events::, keyboard_types::, dpi::, wgpu::, kurbo::, peniko::, parley::, fontique:: and cosmic_text::, with an allowlist of raw_window_handle (the named exception with its price), serde and cursor_icon (allowed 1.x).",
      "pros": [
        "All of B's Stable guarantees, with a sanctioned outlet for the long tail: custom a11y roles, external GPU content, embedders",
        "The version in the module name makes the lack of a promise explicit, which matches the Evolving tier semantics and Slint's shipped precedent",
        "Closes B's main gap, custom widgets needing roles beyond the 33, without widening Stable",
        "Escape-hatch usage shows which roles to promote into owned SemanticsRole, which matches P9's 'second consumer' logic",
        "The enforcement is mechanical, a lint-style gate, which fits AGENTS.md's 'make rules types, not reviews'"
      ],
      "cons": [
        "More surface to maintain: each escape module is re-pinned on each release train",
        "The a11y extension hook must be designed so it cannot corrupt the owned tree, for example applied last and only to properties the owned model leaves unset. This is design work",
        "The closure check is a hypothesis: whether cargo-public-api output covers the transitive closure is unverified and needs a probe before it is made a gate"
      ],
      "cost_now": "Medium, B's cost plus a small increment: the wgpu module is already planned (:314), the a11y hook is a single callback type, and the gate is one xtask check plus a CI step in `checks`.",
      "cost_later": "Low for Stable, where only a raw-window-handle 0.7 forces a major. Moderate and predictable for Evolving: one re-pin per upstream major, allowed by the tier's own rules.",
      "reversibility": "Good. Escape modules are Evolving and can be dropped or renamed in any minor. The Stable part has option B's reversibility.",
      "fits_plan": "Best fit. It keeps P10 as rephrased, P9 and the :314 wgpu decision, gives the tier model a concrete use for the Evolving tier, and amends only :292 (owned vocabulary plus hook instead of a 1:1 mirror) and :102 (serde and cursor-icon on the allowed list, raw-window-handle the sole named exception)."
    }
  ],
  "recommended": "D",
  "rationale": "I confirm that raw-window-handle should be the only named P10 exception. It is the one pre-1.0 crate that passes the cadence test: 0 breaking releases since 0.6.0 in 2023-10, the v0.7 milestone closed empty on 2026-02-10, and wgpu 30, winit 0.30 and bevy_window all depend on ^0.6.\n\nThe exception should cover only the HasWindowHandle/HasDisplayHandle bounds and the #[non_exhaustive] HandleError. The ADR should record the price: raw-window-handle 0.7 would force a flui-platform-api major. That means changing crates/flui-platform/src/window.rs:196, which currently returns the RawWindowHandle enum directly, to a HasWindowHandle bound.\n\nserde and cursor-icon are 1.x crates with 0 breaking releases, so they belong on an 'allowed 1.x' list, not the exception list. That corrects the architecture doc at :102, which lists serde next to raw-window-handle as an exception.\n\nOn AccessKit, I recommend against the doc's 1:1 mirror (:292). AccessKit had 13 breaking releases from 2024-01 to 2026-09 and has no #[non_exhaustive] on Role or Action. Its changelog shows roles being renamed and dropped (StaticText->Label, InlineTextBox->TextRun, ToggleButton, the deprecated roles). A 182-role mirror would take on that churn and duplicate FLUI's existing 33-role SemanticsRole plus flags. The better Stable vocabulary is the owned one:\n- Make SemanticsRole and SemanticsAction #[non_exhaustive] and move them into flui-protocol.\n- Keep the AccessKit mapping in the internal accesskit_translation.rs.\n- Pin that mapping with an exhaustive match test (no wildcard arm), so each accesskit bump becomes a compile error in one internal file rather than a FLUI major.\n\nThe same reasoning applies to input. ui-events had 4 breaking releases in 9 months, and its `pub use keyboard_types::*` pulls keyboard-types and dpi in with it. It should be wrapped in owned pointer and key types that copy the W3C names from keyboard-types.\n\nwgpu (11 breaking releases), kurbo, peniko, parley, fontique and cosmic-text (3 to 11 each) never appear in Stable.\n\nD is B plus Evolving escape modules named for their version: gpu behind a `wgpu-30` feature, an accesskit role/property hook, and ui-events conversions. It also moves flui-testing's accesskit re-exports and android_activity into Evolving.\n\nThese escape modules close B's one real gap: custom widgets that need roles outside the 33. They do this without widening Stable, and they follow Slint's shipped pattern, where raw-window-handle-06 is stable and unstable-wgpu-30 and unstable-fontique-011 are not.\n\nTo enforce all this, add a public-API closure check with a pattern denylist and an allowlist of raw_window_handle, serde and cursor_icon. Probe first that cargo-public-api actually reports the transitive closure; that is still unverified.\n\nUnverified points:\n- How many NamedKey/Code variants keyboard-types has; this sets the cost of owning the input types.\n- The shape of Slint's accessibility API.\n- Anything that needs a build. I ran no build or public-api snapshot, because this task was read-only."
}
```

## judge_q3_p10_exceptions_engineer

```json
{
  "choice": "D",
  "confidence": 0.78,
  "reasons": [
    "I checked the code facts the choice rests on. crates/flui-platform/src/window.rs:196 returns the RawWindowHandle enum directly. SemanticsRole (crates/flui-semantics/src/role.rs:32) and SemanticsAction (action.rs:23) have no #[non_exhaustive]; grep found no attribute in either file. accesskit_translation.rs has wildcard `_ => None` arms at :316 and :359, so a new AccessKit variant is dropped silently today. The exhaustive-match pin in B and D is new, needed work, not a restatement of what exists.",
    "raw-window-handle is correctly the only named exception. It has had 0 breaking releases since 0.6.0 (2023-10), its v0.7 milestone closed empty, and wgpu 30, winit 0.30 and bevy_window all depend on ^0.6. serde and cursor-icon are 1.x with 0 breaks, so they belong on an allowed list, not the exception list. That corrects architecture doc :102.",
    "A 1:1 AccessKit mirror (option A) moves the churn instead of bounding it. AccessKit's changelog renames and drops roles (StaticText->Label, InlineTextBox->TextRun, ToggleButton dropped, deprecated roles dropped). It had 13 breaking releases in 2024-2026, and its Role and Action enums are exhaustive. A mirror would also duplicate FLUI's existing 33-role vocabulary plus flags and add about 150 variants that nothing produces, which P9 forbids.",
    "Option C cannot satisfy the H3 Stable freeze. The cadence data is about 5 accesskit, 4 wgpu and several ui-events breaking releases a year. On top of that, ui-events' `pub use keyboard_types::*` pulls keyboard-types and dpi into the public contract without anyone deciding to.",
    "D over B: the Stable guarantees are the same, and D adds a sanctioned, version-named outlet in Evolving. That is Slint's shipped precedent (raw-window-handle-06 is stable; unstable-wgpu-30 and unstable-fontique-011 are not), and it matches doc :314's flui_sdk::gpu plan. It covers custom-role and embedder needs without widening Stable.",
    "Migration is cheap before 1.0 and is confined to about 16 re-export sites plus one trait method. An exhaustive match over accesskit::Role and Action is only possible because accesskit itself leaves out #[non_exhaustive], so the pin is mechanically sound today."
  ],
  "conditions": [
    "Amend architecture doc :292 to say: owned #[non_exhaustive] SemanticsRole and SemanticsAction (plus flags) in flui-protocol; use AccessKit or ARIA names where the concept exists; do not promise a 1:1 mirror. Amend :102 to name raw-window-handle as the sole exception and list serde and cursor-icon as allowed 1.x crates.",
    "The tier ADR states the price of the exception: raw-window-handle 0.7 forces a flui-platform-api major. It also limits the exception to HasWindowHandle/HasDisplayHandle bounds and HandleError. Replace window.rs:196 `fn raw_window_handle(&self) -> RawWindowHandle` with a HasWindowHandle-based signature.",
    "Replace the wildcard arms at accesskit_translation.rs:316 and :359 with exhaustive matches that have no `_` arm, so every accesskit bump fails compilation in that file. If accesskit ever adds #[non_exhaustive] to Role or Action, this pin becomes impossible. Replace it then with a variant-count or round-trip test and record that in the ADR.",
    "Land each Evolving escape module (gpu `wgpu-30`, `a11y::accesskit_025` hook, `input::ui_events_03`) only together with the production consumer that needs it, or with a named follow-up. Under AGENTS.md's unwired-surface rule, the a11y hook must not ship speculatively. Specify the hook as applied last, touching only properties the owned model leaves unset, and never able to change node identity or tree structure.",
    "Probe cargo-public-api first: confirm it reports upstream types that reach the API through re-exported items and fields, including ui-events' `pub use keyboard_types::*`. Only then make the denylist/allowlist check a `cargo xtask` command plus a step in the `checks` CI job. If the probe fails, fall back to a rustdoc-JSON walk.",
    "Before committing to owned Key/NamedKey/Code types, count keyboard-types' NamedKey and Code variants. Generate the owned types with a From-conversion test, or curate a #[non_exhaustive] subset with an explicit Unidentified fallback. Do not hand-copy them.",
    "Move flui-testing's a11y re-exports (a11y.rs:36) and flui-app's `pub use android_activity` (lib.rs:116) out of any Stable path in the same migration wave. Otherwise the Stable closure check fails on day one."
  ]
}
```

## judge_q3_p10_exceptions_ecosystem_author

```json
{
  "choice": "D",
  "confidence": 0.8,
  "reasons": [
    "Keeping upstream types out of Stable matters most to an app author at H3 and later, because it is what makes cargo-semver-checks gating realistic. accesskit had 13 breaking releases from 2024 to 2026 and marks neither Role nor Action #[non_exhaustive]. ui-events had 4 breaking releases in about 9 months, and its keyboard module does `pub use keyboard_types::*`. wgpu had 11. Any of these in a Stable signature forces a FLUI major several times a year. That rules out C.",
    "A to a third-party package author: a 182-role mirror would copy AccessKit's renames and removals (StaticText to Label, InlineTextBox to TextRun, ToggleButton dropped, deprecated roles dropped) straight into a frozen enum. It would also create a second vocabulary next to FLUI's existing SemanticsRole (crates/flui-semantics/src/role.rs:32), which has 33 roles and treats interactive kinds as flags. A plugin author then has to learn both. Declining A and amending :292 is correct.",
    "B alone leaves a real gap for authors building custom widgets outside the repo: grid, tree or math roles, external GPU content and embedders. Without an outlet they would fork FLUI or depend on internal crates, which is worse for stability than an explicitly version-suffixed Evolving module. D gives that outlet, and it matches Slint 1.x, whose only unprefixed upstream feature is raw-window-handle-06 while wgpu, winit and fontique sit behind unstable-*-NN features.",
    "I agree raw-window-handle should be the only named exception. It has had 0 breaking releases since 0.6.0, the v0.7 milestone closed empty, and it is the ecosystem's interop currency. Verified: crates/flui-platform/src/window.rs:196 returns `RawWindowHandle` itself, which should become a HasWindowHandle bound. serde and cursor-icon are 1.x crates that meet the cadence rule, so they belong on an allowed list, not the exception list.",
    "Verified: crates/flui-engine/src/lib.rs:229 has `pub use ::wgpu`, which must move to Evolving. SemanticsRole and SemanticsAction carry no #[non_exhaustive]; adding it now, pre-1.0, costs little. Also verified, and not stated by the architect: accesskit_translation.rs matches with a `_ => None` wildcard at :316 and :359. The mapping is therefore not pinned today, so the exhaustive-match pin is new work, not an existing safety net."
  ],
  "conditions": [
    "Escape modules must carry the upstream major in their path or feature name (for example `wgpu-30` and `accesskit_025`), be documented as Evolving with no promise across upstream majors, and never be re-exported from Stable crates or the facade prelude.",
    "The a11y extension hook may only add properties or roles that the owned model leaves unset, and it runs last in translation. A test must show it cannot override FLUI-owned name, role, actions, focus or tree structure.",
    "Replace the `_ => None` wildcards in crates/flui-semantics/src/accesskit_translation.rs (:316, :359) with exhaustive matches over accesskit::Role and accesskit::Action, so an accesskit bump fails to compile in one internal file.",
    "Make SemanticsRole and SemanticsAction (and every owned input enum) #[non_exhaustive] before the Stable freeze. Use AccessKit or ARIA names where the concept exists, as a guideline, not a 1:1 contract. Promote roles from the escape hatch into owned roles when a second consumer appears.",
    "Owned input types: generate NamedKey and Code from keyboard-types' W3C lists, or start with a curated subset plus an `Unidentified` or `Other` fallback. Measure the variant count before committing to the cost.",
    "The tier ADR states raw-window-handle as the sole named exception, exposed only through the HasWindowHandle/HasDisplayHandle bounds and HandleError, together with its price: raw-window-handle 0.7 forces a flui-platform-api major. It lists serde 1.x and cursor-icon 1.x separately as allowed upstream crates. It amends architecture doc :102 and :292.",
    "Before the public-API closure check becomes a gate, probe it on one crate with a planted `pub fn f() -> accesskit::Role` and confirm the check flags it. If cargo-public-api does not report upstream paths, use rustdoc JSON instead. The check must be a `cargo xtask` command plus a step in the CI `checks` job.",
    "Move android_activity (flui-app/src/lib.rs:116) and the flui-testing a11y re-exports (a11y.rs:36) into Evolving platform or a11y modules. Stable test helpers assert on owned SemanticsRole and SemanticsAction."
  ]
}
```

## judge_q3_p10_exceptions_owner

```json
{
  "choice": "B",
  "confidence": 0.72,
  "reasons": [
    "I agree with the architect on the diagnosis, and I checked the key facts myself. The doc's 1:1 AccessKit mirror (flui-global-architecture.md:292, restated at :691 and :725) inherits AccessKit's churn: 13 breaking releases in 2024-2026, roles renamed or dropped, and no #[non_exhaustive] on Role or Action. FLUI already owns a smaller vocabulary: SemanticsRole at crates/flui-semantics/src/role.rs:32 and SemanticsAction at action.rs:23, neither #[non_exhaustive] today. accesskit_translation.rs already maps between the two (:299 semantics_action_for). So the right Stable vocabulary is the owned one plus the internal mapping, not a 182-variant mirror.",
    "raw-window-handle is confirmed as the sole named exception. It is the only pre-1.0 crate here with 0 breaks since 0.6 (2023-10), and its v0.7 milestone closed empty. serde and cursor-icon are 1.x crates with no breaks, so they belong on an 'allowed 1.x' list rather than the exception list. That corrects :102. The doc's :482 already words rwh as the single named exception.",
    "I pick B over D because of the sole-owner and WIP constraints. D's extras are an accesskit_025 role/property hook and a ui_events_03 conversion module. Neither has a consumer today, so both are exactly the unwired pub surface that AGENTS.md calls the repo's most common defect and that P9 removes. The a11y hook also needs its own design work: it must not corrupt the owned tree. Each Evolving module adds a re-pin for every upstream major, roughly 5 accesskit and 4 ui-events releases a year, all on one maintainer.",
    "D's real advantages over B can be added later without breaking anything, because Evolving modules can be added in any minor. The doc already decided wgpu interop in Evolving flui_sdk::gpu (:314), and B's fits_plan keeps that decision. So B already includes the one escape hatch that has a real consumer. B's reversibility is good in both directions.",
    "C is ruled out. It contradicts P10 and the H3 freeze, and re-exports become a breaking migration once consumers exist. egui and Masonry make no stability promise, so they are not precedents for a Stable tier."
  ],
  "conditions": [
    "Amend :292, :691 and :725 in the tiers ADR. The Stable vocabulary is the owned SemanticsRole plus flags plus SemanticsAction, made #[non_exhaustive] before the H3 freeze. The rule is 'AccessKit or ARIA names where the concept exists', a naming guideline and not a 1:1 contract.",
    "Pin the AccessKit mapping with an exhaustive match over accesskit::Role and accesskit::Action, no wildcard arm, in the internal translation module. Each accesskit bump must then fail to compile in one file, not drop a role silently.",
    "The tiers ADR names raw-window-handle 0.6 as the only exception, exposed only through the HasWindowHandle/HasDisplayHandle bounds and HandleError, with the price stated: 'rwh 0.7 means a flui-platform-api major'. Replace `fn raw_window_handle(&self) -> RawWindowHandle` (crates/flui-platform/src/window.rs:196) before any freeze. serde 1.x and cursor-icon 1.x go on an 'allowed 1.x' list.",
    "Keep wgpu only in the Evolving flui_sdk::gpu module, per :314, using version-suffixed naming such as a wgpu-30 feature. Remove `pub use ::wgpu` (flui-engine/src/lib.rs:229) from any path reachable from Stable.",
    "Add D's other escape modules only on demand, each named for its version and each with a named consumer: the accesskit role/property hook when a custom widget needs a role outside the owned set, the ui-events conversions when an embedder needs them. Until then, missing roles are added to SemanticsRole as additive minors.",
    "Owned input types in flui-platform-api use keyboard-types' W3C names. Before committing, count the NamedKey/Code variants in keyboard-types 0.8 (not yet verified) and decide between generating the types and a curated #[non_exhaustive] subset. The ui-events glob re-export (`pub use keyboard_types::*`) must not leak into any Stable path.",
    "Before turning the P10 gate into a CI check, probe whether cargo-public-api reports the transitive closure, in a throwaway crate. If it does, add one xtask check folded into `cargo xtask checks`: a denylist of accesskit/ui_events/keyboard_types/dpi/wgpu/kurbo/peniko/parley/fontique/cosmic_text over flui, flui-platform-api and flui-protocol, with an allowlist of raw_window_handle, serde and cursor_icon.",
    "The accesskit re-exports in flui-testing (a11y.rs:36) and the android_activity re-export in flui-app (lib.rs:116) stay internal or Evolving. Stable test helpers assert on the owned SemanticsRole and SemanticsAction.",
    "Schedule this work within existing migration waves (W0-W8) rather than as a new WIP item. The #[non_exhaustive] change and the exhaustive pin test are small and can go first. The owned input types go in the flui-platform-api extraction wave."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "Making SemanticsRole #[non_exhaustive] and moving it to flui-protocol removes the compile-time pin FLUI has today on the outbound direction. #[non_exhaustive] applies across crates, so the internal translation crate would need a `_` arm in its match over SemanticsRole. A role added to flui-protocol later would then be silently left unmapped to AccessKit. The chosen option says the pin works the other way, which is wrong.",
      "evidence": "crates/flui-semantics/src/accesskit_translation.rs:141-159 has `fn explicit_role(role: SemanticsRole) -> Option<Role>` with an exhaustive `match role`, same crate, no wildcard. crates/flui-semantics/src/role.rs:31-32 has `#[repr(u32)] pub enum SemanticsRole` with no non_exhaustive. The same applies to SemanticsAction (action.rs:22-23) wherever the outbound action mapping lives.",
      "severity": "major",
      "fix": "Keep both directions pinned by a test rather than by match exhaustiveness. Give the owned enums a generated `const ALL: &[SemanticsRole]` (or EnumIter), and add a test asserting that every variant except None maps to Some(accesskit role), and that every SemanticsAction has an inbound AccessKit source or a documented 'FLUI-only' entry. Record this in the tier ADR as the price of #[non_exhaustive]."
    },
    {
      "problem": "The 'exhaustive match over accesskit::Role, no wildcard' pin does little in practice. No production code matches over accesskit::Role, because the outbound mapping matches on SemanticsRole. A new AccessKit role is therefore irrelevant, since FLUI never emits it. A renamed or removed role already fails compilation in explicit_role. A 182-arm match over accesskit::Role would be test-only boilerplate. Only the Action pin, which is the inbound direction, catches real drops.",
      "evidence": "accesskit-0.25.0/src/lib.rs:61-62 `#[repr(u8)] pub enum Role` has no non_exhaustive, and I counted 182 variants. accesskit_translation.rs:299-317 `semantics_action_for(action: accesskit::Action)` ends with `_ => None` (:316). Action has 23 variants (lib.rs:289). The only other wildcard is :359, and it matches accesskit::ActionData, not Role or Action.",
      "severity": "minor",
      "fix": "Limit the no-wildcard pin to semantics_action_for (:316) and semantics_action_args_for (:359, over ActionData, where a new payload kind would otherwise drop silently). Drop the Role pin, or replace it with the ALL-variants outbound test from the previous item."
    },
    {
      "problem": "The owned input types cannot be pinned against keyboard-types. NamedKey and Code are #[non_exhaustive] upstream, so a `From<keyboard_types::NamedKey>` in the internal crate must contain a wildcard, and new upstream keys will silently become Unidentified. The judges' 'generate + From-conversion test' condition does not give the compile-error guarantee claimed for AccessKit. The size question the options left open is now measured: NamedKey has about 307 variant lines and Code about 216.",
      "evidence": "keyboard-types-0.8.3/src/named_key.rs:23-24 and code.rs:24-25 both have `#[non_exhaustive] pub enum`. `grep -cE '^    [A-Z][A-Za-z0-9]*,?$'` counted named_key.rs:307 and code.rs:216. Both counts are approximate, since the pattern can include doc-adjacent lines.",
      "severity": "minor",
      "fix": "Generate the owned enums from keyboard-types' source in an xtask. Add a test that round-trips every generated variant, and a CI or xtask diff check that flags new upstream variants on a keyboard-types bump, rather than relying on match exhaustiveness. Or ship a curated #[non_exhaustive] subset with an explicit Unidentified fallback, and state in the ADR that new W3C keys arrive as Unidentified until adopted."
    },
    {
      "problem": "The enforcement gate D depends on is not available on the dev host. Neither cargo-public-api nor cargo-semver-checks is installed, so the closure-check probe the judges require has not been run and cannot run as-is. Adopting it means adding a tool to `cargo xtask doctor` and to CI. cargo-public-api also needs a nightly rustdoc-JSON toolchain, which is my recollection and a hypothesis, while rust-toolchain.toml pins stable.",
      "evidence": "`which cargo-public-api cargo-semver-checks` returned 'no cargo-public-api in (...)' and 'no cargo-semver-checks in (...)'.",
      "severity": "minor",
      "fix": "Keep the gate conditional, as the judges said. Probe it first in the scratchpad with a planted `pub fn f() -> accesskit::Role`. If the nightly requirement is confirmed, prefer a rustdoc-JSON walk inside xtask that reuses whichever toolchain H3's cargo-semver-checks already needs."
    }
  ]
}
```
