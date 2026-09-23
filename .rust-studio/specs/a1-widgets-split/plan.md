# A1: split flui-widgets into widgets / text-editing / scrolling / navigation

Status: APPROVED by Master (2026-09-23), with the decisions below. Tracking issue: #1272.
Baseline for the analysis: d7c7932f. PR 0 starts from 25c8a19b (after #1261, B2),
where the text-editing map was re-checked: #1261 added ~1.3k lines to
`text/{controller,editable_text}.rs` and no new boundary crossings. The only new
dependency, `unicode-segmentation`, moves with text-editing.

## Decisions (Master, on approval)

- **(i) Overlay: public API, not doc(hidden).**
  - The overlay mutation surface becomes real, documented API (Flutter's
    `OverlayState`/`OverlayEntry` are public, and material builds menus and
    tooltips on it).
  - The compiler decides which methods go public, not a regex. Result: 12
    items. They are `InsertPosition`, `OverlayHandle::{new, insert, rearrange,
    is_mounted}`, `Overlay::new` and `OverlayEntry::{new, remove,
    mark_needs_build, set_opaque, set_maintain_state, is_attached}`. That
    closes ADR-0036's deferred public constructor. `is_mounted` (the overlay)
    and `is_attached` (an entry in its list) keep distinct names because
    they answer different questions.
  - Bookkeeping-only internals (`OverlayEntry::id`, `element_id`) stay private;
    their call sites move to `is_same`/handle comparisons.
  - The revision is a new ADR ("Public overlay mutation API") with
    `Supersedes: ADR-0036 §1`, plus a one-line footnote in ADR-0036. It is not
    an addendum.
  - `OverlayScope`/`Theater`/`OverlayShared`/`OnstagePlan` stay private; tests
    reach them through `testing::overlay_probe`.
- **(b)/(d) One explicit seam, not scattered `#[doc(hidden)] pub`:**
  - `#[doc(hidden)] pub mod __private` in flui-widgets holds
    `enclosing_focus_parent`, `install_rect_provider`, `SaltingChildKey` and
    `generic_render_view_element`.
  - The macro is `#[macro_export]`'d as `__generic_render_view_element` and
    re-exported there.
  - `AnchoredBox` joins it.
  - Its module docs say it is for `flui-*` crates only, with no semver
    guarantee.
  - port-check `SEAM/widgets-private` forbids any importer outside
    `crates/flui-*`, the macro name included.
- **(c)** `axis_direction_from_axis_reverse_and_directionality` becomes `pub`
  (it is public in Flutter).
- **(e)** `testing::harness` sits behind the `testing` feature. Feature-gated
  code has to be shown to run in CI (a `Starting N` line from the right job).
- **(f)** `testing::overlay_probe` is read-only.
- **(ii)** Order: text-editing, then scrolling, then navigation.
- Real `--timings` before/after are a required part of PR 4. The LOC model
  above is an estimate and never reported as a measurement.
- The first PR of the series (PR 0) carries the `full-ci` label. It is the
  live proof of #1267's re-run path: the pull_request run was re-run, plan saw
  the label, and the `ci` check took the heavy result.

## Open question (not for this series)

- `app/widgets_app.rs` goes to flui-navigation for now, because it hosts the
  Navigator. Should `WidgetsApp` live in the app layer instead?

## Deviations recorded during PR 0

- Base docs that link items moving downstream (for example
  `axis_direction_…` → `CustomScrollView`) stay as links in PR 0, where they
  still resolve. Each extraction PR converts the ones its move breaks, and
  its `cargo doc -D warnings` proves it.
- `pub mod __private` carries a `PORT-CHECK-OK-SP4` marker (trigger 11:
  there are no workspace consumers until PR 1). PR 1 removes the marker.
- `testing::harness` needs `flui_platform::traits::PlatformTextInput`: the
  `testing` feature now enables an optional `flui-platform` dependency (L2,
  downward; flui-interaction already depends on it normally).
- `scripts/check-panic-policy.sh` excluded only `NAME/` for a gated `mod
  NAME;` when both `NAME.rs` and `NAME/` exist (Rust 2018 layout). It now
  excludes both, with a self-test. That self-test is not on the merge path
  (no CI step runs `--self-test`); this was already the case before.

## 1. Module map

Partition rule (by file):

- **flui-scrolling**: `scroll/**`
- **flui-navigation**: `navigator/**` plus `app/widgets_app.rs` (WidgetsApp builds the Navigator)
- **flui-text-editing**: `text/{editable_text,controller,text_field}.rs`
- **flui-widgets (base)**: everything else, including `text/{text,rich_text,default_text_style}.rs`,
  `app/{media_query,safe_area,inherited_theme}.rs`, `overlay/`, `interaction/`, `testing.rs`

| crate | prod LOC | in-crate test LOC | tests/ LOC | total | tests/ files |
|---|---:|---:|---:|---:|---:|
| flui-widgets (base) | 26 722 | 9 746 | 11 340 | 47 808 | 64 |
| flui-navigation | 14 761 | 13 963 | 5 475 | 34 199 | 4 |
| flui-scrolling | 6 355 | 1 412 | 8 624 | 16 391 | 7 |
| flui-text-editing | 3 027 | 3 949 | 551 | 7 527 | 1 |
| **total** | 50 865 | 29 070 | 25 990 | 105 925 | 76 |

Public root re-exports: 284 in total. Moving: 41 scrolling, 40 navigation (37 navigator + 3
WidgetsApp/AppBuilder/WidgetsAppState), 6 text-editing (EditableText, EditableTextState,
SubmitCallback, TextEditingController, RawTextField, RawTextFieldState).

No moved file has `#[cfg(feature = ...)]`, so images, asset-images, network-images,
signals and serde all stay in base only.

Consumers (identifier scan; prod means the lib target, tests means dev only):

| consumer | scrolling | navigation | text-editing |
|---|---|---|---|
| flui-material | prod (3 files) | prod (6) | prod (2, `text_field.rs`) |
| flui-cupertino | — | prod (5) | — |
| flui-app | tests only (ListView) | **prod** (`NavigatorCommand`, ui_realm/commands.rs) | tests only (presentation_text_input) |
| flui-localizations | — | — | — |
| facade `src/` | via `pub use flui_widgets as widgets` + prelude glob | same | same |
| examples/ | 5 files | 3 files | 3 files |
| hot-reload-counter-logic, web-counter | — | — | — |

Module-path users that will break: `flui_widgets::scroll::` appears only in a
flui-rendering doc comment (scroll_position.rs), and `flui_widgets::text::…` only in
material/src/text_field.rs. Everything else imports root items.

## 2. Crate boundaries and dependency graph

```
flui-widgets (L6, base)
   ▲        ▲          ▲
   │        │          │
flui-scrolling  flui-navigation  flui-text-editing   (all L6, each → flui-widgets only)
   ▲  ▲         ▲   ▲   ▲            ▲
   │  └─────────┼───┼───┼────────────┤
material(L7) ───┘   │   │            │     material → all three
cupertino(L7) ──────┘   │                  cupertino → navigation
app(L9) ────────────────┘  (+ dev: scrolling, text-editing)
flui(L10) → all four
```

The graph is acyclic. The three new crates do not depend on each other: text-editing has
no scroll or navigator reference in code, and the one navigator-test reference to text is
`crate::Text`, which stays in base.

**The only code cycle today is interaction ↔ navigator, via `AnchoredBox`.** It is
`pub(crate)` in `navigator/subtree.rs:215` and used by base (interaction) and by
text-editing. Fix: move it into base. No trait or inversion is needed.

The static check also found that the new crates use **crate-private base surface**. Each
item has to become reachable from another crate before it can move:

| item (today) | used by | proposal |
|---|---|---|
| `overlay` module is private; `InsertPosition` (pub(crate) enum); `OverlayHandle::{insert, rearrange}`; `OverlayEntry::{set_opaque, mark_needs_build, is_attached}` | navigation (hero_flight, binding, navigator.rs) | **Public API.** These are public in Flutter (`OverlayState.insert/rearrange`, `OverlayEntry.opaque/markNeedsBuild/mounted`), and material needs them for menus and tooltips. Export `InsertPosition` at root. **API-GATE item.** |
| `interaction::{enclosing_focus_parent, install_rect_provider}` (pub(crate)) | text-editing | `#[doc(hidden)] pub`: framework wiring, not author API |
| `localization::axis_direction_from_axis_reverse_and_directionality` (pub(crate)) | scrolling (5 files) | `pub`: Flutter-public `getAxisDirectionFromAxisReverseAndDirectionality` |
| `RepaintBoundary::salting_child_key` (pub(crate)) | scrolling (sliver_list) | `#[doc(hidden)] pub` |
| `support::generic_render_view_element` (private mod) | scrolling (3 files) | `#[doc(hidden)] pub` via root re-export |
| `test_harness` (`#[cfg(test)]` private mod: Harness, mount, mount_with_ime, mount_with_capabilities, dispatch_*) | in-crate tests of navigation and text-editing | move to `testing::harness` under `cfg(any(test, feature = "testing"))` |
| overlay internals in navigator tests (`entry_ids`, `is_mounted`, `Theater`, `OverlayScope`, `OverlayShared`, `OnstagePlan`, `OverlayEntryViewState`) | 4 navigator test files | read-only `testing::overlay_probe` (feature `testing`); the tests themselves are unchanged |

Checked and clean:

- 0 inherent impls across the new boundaries.
- 0 orphan-rule candidates.
- No `pub(crate)` fields or methods used across boundaries other than those listed above.

The scan was heuristic, so the compiler still has the final word in PR 0.

## 3. Layers, facade and imports

**docs/workspace-layers.toml**

- Three `[[member]]` entries at layer 6. Suggested `release_role = "support"`, the same as
  flui-widgets.
- Three `[[same_layer_edge]]` entries (`flui-{scrolling,navigation,text-editing}` → `flui-widgets`).
  There is precedent: `flui-widgets → flui-testing`. The alternative is renumbering
  L7–L10 to fit a new layer, which touches every upper member. Not worth it.
- `[[checkout_only_dev]]` for each new crate's `flui-widgets = { path, features = ["testing"] }`
  dev edge, mirroring the existing flui-widgets entry.

**Facade `flui`**

- `pub use flui_widgets as widgets;` becomes a merged module:
  `pub mod widgets { pub use flui_widgets::*; pub use flui_scrolling::*; pub use flui_navigation::*; pub use flui_text_editing::*; pub mod prelude {…} }`.
  The explicit inner `prelude` shadows the glob-vs-glob ambiguity. `flui::widgets::X` keeps
  resolving for every moved X.
- `flui::prelude` stays simple: it globs the four crates' preludes, and each new crate
  ships `prelude` with its author-facing items.
- `src/testing.rs`: `widgets` gains the harness re-export.
- No new facade features are needed, since moved code has no feature cfgs.

**Import migration (breaking, allowed by the mandate)**

`flui_widgets::{EditableText, ListView, Navigator, …}` becomes
`flui_{text_editing,scrolling,navigation}::…`. Base cannot re-export downstream crates, so
no compatibility shim is possible without a cycle.

At most about 30 files change (union of the per-crate hits; files may overlap):

- material: ≤11
- cupertino: 5
- app: 1 prod file + tests
- examples: ≤11
- 2 doc mentions in lower crates

Users of the `flui` facade see no change.

## 4. Build-time and size effect (static estimate; measurement promised in PR 4)

Model: prod LOC of lib targets recompiled when one file changes, using the declared
dependency graph. flui-app's lib is not rebuilt for scrolling or text-editing edits,
because it uses those only in tests.

| edit in | rebuilt today (`cargo check`, lib targets) | rebuilt after | reduction |
|---|---|---|---|
| text-editing | widgets 50.9k + localizations 0.3k + material 26.9k + cupertino 4.3k + app 52.1k + flui 0.4k = **134.9k** | text-editing 3.0k + material 26.9k + flui 0.4k = **30.3k** | **−78%**; base, scrolling, navigation, localizations, cupertino, app, hot-reload examples not rebuilt |
| scrolling | 134.9k | scrolling 6.4k + material + flui = **33.7k** | **−75%** |
| navigation | 134.9k | navigation 14.8k + material + cupertino + app + flui = **98.5k** | **−27%**; app depends on it in prod |
| base | 134.9k | 134.9k | 0; the serial hop shrinks from 50.9k to 26.7k base → 14.8k navigation, with scrolling and text-editing typechecked in parallel (rustc's frontend is serial per crate); +3 rustc invocations |

- **Inner test loop** (`cargo nextest run -p <crate>`) after editing EditableText:
  - Today it compiles the flui-widgets lib (50.9k), its cfg(test) unit binary (≈80k) and
    widgets_it (26k), about 157k LOC in total.
  - After, it compiles flui-text-editing lib, unit and it binaries, about 10.5k LOC.
    That is **about 15× less**.
- **Artifacts, measured on existing worktree targets** (debug, CARGO_INCREMENTAL=0):
  - Today: libflui_widgets rlib 55–62 MiB, rmeta 4–5 MiB; unit-test binary 18 MiB;
    widgets_it 25 MiB.
  - After, a text-editing edit relinks about a 3 MiB rlib instead of 56 MiB. This is a
    proportional estimate and has not been verified.
- **CI fast lane (#1267)**: `change_scope` includes declared dev-dependents. A text-editing
  PR's scope becomes text-editing + material + app (its tests) + flui + web-counter,
  instead of flui-widgets + all 8 dependents.

Caveats:

- LOC is a proxy for compile time.
- Monomorphization of base generics moves into the new crates.
- Base edits gain nothing.
- Real numbers need a build slot. PR 4 posts `cargo build --timings` before and after, on
  one machine with jobs=6: a cold build, then an edit-one-file build in each crate.

## 5. PR breakdown

Every PR runs the script gates before push. Each extraction PR registers its crate
everywhere in the same PR, so no gate goes red between PRs.

**Registration checklist** (per new crate):

- Cargo.toml `members`, `default-members` and `[workspace.dependencies]`, plus Cargo.lock
- docs/workspace-layers.toml (member, same_layer_edge, checkout_only_dev)
- docs/crates.md, docs/testing.md, README crate table
- justfile `active_crates`
- ci.yml `FM_GROUP_*`: each member exactly once; the aggregator test enforces this
- .github/CODEOWNERS, .coderabbit.yaml
- check-workspace-inventory.sh passes

- **PR 0: prep inside flui-widgets. No new crate, no behaviour change.**
  - (a) Move `AnchoredBox` into base, which removes the only cycle.
  - (b) Make the crate-private items from §2 reachable. The overlay items become public API
    only once the API-GATE is approved; otherwise they are `#[doc(hidden)] pub`.
  - (c) Move `test_harness` into `testing::harness`, and add `testing::overlay_probe`.
  - (d) Rewrite every `crate::` path in the three partitions to the path the file will use
    from outside, so later PRs are `git mv` + `s/crate::/flui_widgets::/`.
  - (e) Turn base docs that link downstream (localization → scroll widgets, layout → SliverList,
    interaction → ScrollController, …) into code spans or re-path them.
  - Proof: gates, CI, and an unchanged test count
    (`cargo nextest list -p flui-widgets | wc -l` before and after).
- **PR 1: extract flui-text-editing** (3 files, 1 IT file). It goes first because it is the
  smallest and the biggest per-edit win, and it validates the recipe.
  - `git mv` keeps history.
  - Consumers: material (2 files), app tests, examples.
  - The facade becomes the merged `widgets` module.
  - port-check `ADR-0037/focus-owner` scope gains `crates/flui-text-editing`.
- **PR 2: extract flui-scrolling** (scroll/**, 7 IT files).
  - port-check `fr036_scope` gains `crates/flui-scrolling/src`.
  - The flui-rendering doc mention gets updated.
- **PR 3: extract flui-navigation** (navigator/** + widgets_app.rs, 4 IT files).
  - port-check `FR-033/widgets`: the sanctioned Navigator downcast sites move. Today's rg
    call there ends in `2>/dev/null || true`, so a missing path silently disables the guard
    (the same fail-open class fixed for `check()` in #1268). It gets the new path plus a
    missing-path VIOLATION.
  - `fr036_scope` and the focus-owner scope gain the crate.
  - Consumers: cupertino, material, app prod.
- **PR 4: facade and prelude finish, docs, measurements.**
  - Final `flui::prelude`.
  - book/src/widgets/catalog.md, docs/architecture.md.
  - Measured `--timings` table as in §4.

Additional risks:

- In-crate navigator tests (14k LOC) become unit tests of flui-navigation. They need
  `flui-widgets = { features = ["testing"] }` as a dev-dependency. That edge is
  checkout-only, like material's.
- A PR that is merged but still open against the old paths gets rename-conflicts. Only
  #1261 touches these files today, and it is a prerequisite.

Not verified:

- Nothing is compiled. The dependency and privacy findings come from a regex scan, so
  PR 0's compiler run is the proof.
- Build numbers are LOC estimates.
- The new crates' rlib sizes are proportional estimates.
