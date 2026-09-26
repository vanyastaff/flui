# Decision panel: q6_facade_default_material

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Facade today is Material-first: D:\\flui\\Cargo.toml:598 `default = [\"material\"]`, :605 `material = [\"dep:flui-material\"]`, :606 `cupertino = [\"dep:flui-cupertino\"]`, :525-526 both are optional normal deps; the comment at :592-597 says the default exists so `cargo add flui` works as the README and crate docs teach.",
    "The facade re-exports the whole crate (D:\\flui\\src\\lib.rs:148-149 `#[cfg(feature = \"material\")] pub use flui_material as material;`) and globs about 40 Material types into `flui::prelude` (src/lib.rs:267-276: Scaffold, AppBar, ElevatedButton, Theme, ThemeData, MaterialApp, TextField, ...). The facade is going to be the Stable tier (D8), so flui-material would be a public dependency of a Stable crate: a Material major bump would force a `flui` major (Cargo SemVer reference: changing a public dependency's major version is a breaking change).",
    "The README promises the default: D:\\flui\\README.md:44-66 ('Choosing a catalog': 'The `flui` facade is **Material-first by default**', a table row `material | **on**`, and `default-features = false` recipes). src/lib.rs:38-64 repeats this in the crate docs and runs a doctest gated on `feature = \"material\"` that uses `flui::material::{AppBar, Scaffold, Theme, ThemeData}`.",
    "`flui create` depends silently on the default: crates/flui-cli/src/templates/counter.rs:22 writes `flui = {source.dependency(\"flui\", &[])}` with no features, but the template body uses Material through the prelude: counter.rs:80 `Theme::new(ThemeData::light(), ...)` and counter.rs:115 `ElevatedButton::new(...)`. With `default = []` the generated project stops compiling unless the template also adds `flui-material` (plus `use flui_material::prelude::*`) in the same PR. basic/empty/widget/hot_reload templates have no Material identifiers (grep count 0).",
    "Examples: 11 `[[example]]` entries in the root Cargo.toml, and 13 `required-features` lines that include \"material\" (examples and tests together: material_demo, counter, todo, ios_demo, sliver_demo, screenshot, multi_window_demo, workload_probe, a11y_probe, lifecycle_probe, ...). `cargo xtask facade-combos` (tools/xtask/src/tasks/facade.rs:19-30,146) builds 7+ feature combinations that are all keyed on material/cupertino; it has to be rewritten, as §7 of the architecture doc says.",
    "Current edges (from `cargo metadata --no-deps`, filtered in Python): `flui -> flui-material (optional)`, `flui -> flui-cupertino (optional)`, `flui-web-counter -> flui`. No package depends on the facade today, so there is no cycle now. The cycle only appears under the rule 'packages depend on flui'.",
    "The architecture doc already decides that packages depend on `flui-sdk`, not on `flui` (scratchpad flui-global-architecture.md:14, D17 at :609). It removes the facade `material`/`cupertino`/`devtools`/`hot` features (:189) and adds a gate 'no core crate names an official package, even optionally' (:178). Its stated reason for removing `material` (:19, :684) is the cycle. My experiment below shows the cycle depends only on the package→facade edge, so the cycle does not by itself force removing a `material` feature once packages sit on flui-sdk. The real reasons are the gate, semver and release-train coupling.",
    "The docs that mention `flui::prelude`/`flui::material` and need updating: docs/FOUNDATIONS.md, docs/getting-started.md, docs/testing.md (grep -rln over docs/*.md; archive and research files are exempt)."
  ],
  "market_precedents": [
    {
      "who": "Flutter 3.47 (Aug 2026)",
      "what": "Material and Cupertino moved out of the SDK into the standalone pub packages `material_ui` / `cupertino_ui` 1.0. Apps add them to pubspec explicitly; `dart fix --apply --code=migrate_design_widgets` rewrites the imports. Old imports are deprecated in a later release and removed in 2027.",
      "outcome_or_lesson": "This is the closest precedent to option 'no Material in the facade, the app adds the package explicitly'. The stated reason is an independent release cadence (weekly package releases without an SDK upgrade), which matches D7/D17. The lesson is to ship a migration tool (`flui migrate`) and a phased deprecation. The fetched docs do not say whether `flutter create` adds material_ui automatically; that is unverified.",
      "source": "https://docs.flutter.dev/release/breaking-changes/material-ui-and-cupertino-ui ; https://flutter.dev/blog/decoupling-material-cupertino"
    },
    {
      "who": "Bevy",
      "what": "The `bevy` meta-crate uses `default = [\"2d\",\"3d\",\"ui\",\"audio\"]` plus named collections `default_app`, `default_platform`, `2d_api`, `3d_api`, `dev`. All the defaults are first-party crates inside the same release train.",
      "outcome_or_lesson": "Batteries-included defaults work when everything they pull in is released in lockstep with the meta-crate. Bevy's defaults are engine subsystems, not an optional design language, so they do not argue for keeping a separately versioned catalog in `flui`'s default. Collection-named features (`default_app`) are a pattern for a separate convenience entry point.",
      "source": "https://raw.githubusercontent.com/bevyengine/bevy/main/Cargo.toml"
    },
    {
      "who": "Dioxus",
      "what": "`dioxus` has `default = [\"launch\",\"devtools\",\"logger\",\"lib\"]`, a `minimal` set, and opt-in renderer features (`desktop`, `web`, `mobile`, `router`, `fullstack`). It has no component or design-system library in its defaults; styled components are third-party (e.g. dioxus-components).",
      "outcome_or_lesson": "A Rust UI facade can default to runtime plus devtools with no design language. The one thing Dioxus defaults on and FLUI plans to drop is `devtools`, and that works for Dioxus because dioxus-devtools does not depend back on `dioxus`.",
      "source": "https://raw.githubusercontent.com/DioxusLabs/dioxus/main/packages/dioxus/Cargo.toml"
    },
    {
      "who": "Slint",
      "what": "Multiple built-in styles (fluent, material, cupertino, qt), chosen by name or the SLINT_STYLE env var. The default was changed from platform-native to Fluent.",
      "outcome_or_lesson": "Some default style is expected for newcomer DX, but Slint keeps the styles in-tree and in-train. Changing the default was a publicised breaking UX change with a blog post: announce the FLUI change the same way.",
      "source": "https://slint.dev/blog/default-native-style-change ; https://docs.slint.dev/latest/docs/slint/reference/std-widgets/style/"
    },
    {
      "who": "Cargo SemVer reference",
      "what": "Re-exporting a dependency's types makes it a public dependency, and bumping its major version is a breaking change for the re-exporter.",
      "outcome_or_lesson": "`pub use flui_material as material` in a Stable `flui` ties Material's majors to flui's majors, which defeats the independent cadence that Flutter 3.47 split the packages out for.",
      "source": "https://doc.rust-lang.org/cargo/reference/semver.html"
    }
  ],
  "constraints": [
    "The cycle claim is conditionally true. Cargo rejects a cycle even when the optional edge is disabled, but only if the package depends on the facade. If Material depends on `flui-sdk` (already decided in D17), a `material` default feature on the facade compiles without a cycle. So the case for removing it must rest on semver/public-dependency coupling, release-train ordering and the 'core never names official' gate, not on the cycle alone. The architecture doc (:19, :684) should be corrected to say this.",
    "Recommended decision (confirms §14 q6, with a corrected reason): `flui` gets `default = []` and no `material`/`cupertino` features. Reasons: (1) a Stable facade re-exporting an Evolving, independently versioned package is a public-dependency semver leak; (2) with `packages/` in a separate workspace (D7), publishing `flui` would have to wait for a `flui-material` release (sdk → material → flui ordering), which couples the train to the package; (3) the D17 gate stays simple ('core never names an official package, even optionally'); (4) Flutter 3.47 is the direct precedent.",
    "Newcomer DX mitigation, required in the same PR: `flui create` (counter template, crates/flui-cli/src/templates/counter.rs:22) must emit `flui-material = <ver>` and `use flui_material::prelude::*;`, and the default template should keep Material so the first run looks the same. The README quick start shows two dependency lines. docs.rs for `flui` gets a 'Design systems' section linking flui-material/flui-cupertino.",
    "Meta-crate alternative (e.g. `flui-kit`/`flui-app-kit` = flui + flui-material): no cycle (probe C). It restores one-line `cargo add`, but it adds a third name to explain, and its version must pin both a Stable crate and an Evolving package, so it would break often. It is worth considering only if the two-line install measurably hurts adoption. Recommend deferring it: `flui create` already covers the one-command path.",
    "Naming `flui` vs `flui-core`: keep `flui` as the catalog-neutral Stable facade. Renaming it to free `flui` for a batteries meta-crate would make the Stable promise sit on a less discoverable name and invert D8. Not recommended.",
    "Examples: options are (a) move them to packages/flui-material/examples (build against the published train, matching D7), or (b) a root dev-dependency on flui-material. Option (b) is cycle-safe even if material → flui (probe D: `cargo check --all-targets` ok) and is trivially safe with material → flui-sdk, but it puts a core→official edge in the dev graph, so the D17 gate must either explicitly exempt dev-dependencies or forbid them. Prefer (a) for Material showcases and keep catalog-free examples (widgets_gallery, colored_box_app, ...) at the root.",
    "The same PR must update README.md:44-66, src/lib.rs:38-64 and 213-276 (the prelude Material half and the doctest), the AGENTS.md `[[example]] required-features` row, `cargo xtask facade-combos`, and docs/FOUNDATIONS.md, docs/getting-started.md and docs/testing.md.",
    "Hypothesis (not verified): whether Flutter 3.47's `flutter create` template adds material_ui automatically; the fetched docs are silent."
  ],
  "experiments_run": [
    "Probe A (scratchpad\\probe-q6\\A): workspace with flui{optional mat, default=[]} and mat→flui. `cargo metadata` gives exit 101, `error: cyclic package dependency: package flui ... depends on itself`, even with the feature off. This reproduces the doc's claim.",
    "Probe B: sdk; flui→sdk + optional mat, `default=[\"material\"]`; mat→sdk. `cargo metadata` exit 0: no cycle with Material as a default feature when packages depend on sdk rather than the facade.",
    "Probe C: meta-crate kit→{flui, mat}, mat→flui. Exit 0: the meta-crate option is cycle-free.",
    "Probe D: flui [dev-dependencies] mat, mat→flui. `cargo metadata` exit 0 and `cargo check -p flui --all-targets` Finished: a dev-dependency cycle is allowed for examples.",
    "`cargo metadata --format-version 1 --no-deps` on D:\\flui, filtered for edges into flui/flui-material/flui-cupertino: only flui→material (optional), flui→cupertino (optional), flui-web-counter→flui.",
    "grep of crates/flui-cli/src/templates/*.rs for Material identifiers: counter.rs uses Theme/ThemeData/ElevatedButton through a feature-less `flui` dependency; the other templates have 0 hits.",
    "grep -c on the root Cargo.toml: 11 [[example]] entries, 13 required-features lines naming \"material\"."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "Catalog-neutral facade: default = [], no material/cupertino features; flui create emits flui-material explicitly",
      "description": "`flui` loses its `material`/`cupertino` features and its `pub use flui_material as material`, and Material leaves `flui::prelude` (src/lib.rs:148-149, 267-276). The counter template (crates/flui-cli/src/templates/counter.rs:22, which today relies on the default at :80/:115) writes `flui-material = <ver>` plus `use flui_material::prelude::*;`, so a newly created app looks the same on first run. The README quick start shows two dependency lines. Material showcase examples move to packages/flui-material/examples; catalog-free examples stay at the root. `cargo xtask facade-combos` is rewritten around the remaining features. Announce it like Slint's default-style change and like Flutter 3.47's migration note.",
      "pros": [
        "Matches Flutter 3.47, the closest precedent: material_ui and cupertino_ui are separate packages, and each app adds them explicitly.",
        "The Stable facade no longer has an Evolving package as a public dependency, so a Material major does not force a `flui` major (Cargo SemVer reference).",
        "The D17 gate stays one simple rule: 'core never names an official package, even optionally'. No exemptions.",
        "Releases are ordered sdk → flui, and packages ship on their own cadence (D7). Publishing `flui` does not wait for a `flui-material` release.",
        "It matches Dioxus: the facade defaults to runtime only and has no design language.",
        "It costs the least to do now, before any external consumers exist (the project is pre-1.0)."
      ],
      "cons": [
        "`cargo add flui` alone no longer gives a styled app, so the newcomer needs a second line or `flui create`.",
        "README.md:44-66, the src/lib.rs:38-64 doctest, docs/FOUNDATIONS.md, docs/getting-started.md, docs/testing.md and the AGENTS.md examples row all have to change in the same PR.",
        "About 11 examples and 13 `required-features` lines have to move or be rewritten, and facade-combos has to be rewritten.",
        "If the template change and the facade change land in separate PRs, a generated project can silently stop compiling."
      ],
      "cost_now": "Medium. One coordinated PR (or a tight pair) covering the facade features and prelude, the counter template, README/lib docs and doctest, 3 docs files, the example moves and the facade-combos rewrite. There is no code migration for external users because none exist.",
      "cost_later": "Low. Material versions independently. A convenience meta-crate (option C) can still be added later without breaking anything.",
      "reversibility": "High. Re-adding an optional `material` feature later is additive and non-breaking, provided Material sits on flui-sdk (probe B shows no cycle then).",
      "fits_plan": "Direct fit. It implements §14 q6, D7, D8 and D17 and the W-phase removal of facade features (architecture doc :189, :178). One correction to the doc: the cycle is not the reason. Probe A shows a cycle only when a package depends on the facade. Probe B shows a `material` default compiles fine when Material depends on flui-sdk. The doc's rationale at :19/:684 should instead cite semver/public-dependency coupling, release-train ordering and the gate."
    },
    {
      "id": "B",
      "name": "Keep `material` as the facade's default feature (Material depends on flui-sdk)",
      "description": "Status quo, moved onto the new graph. Material depends on flui-sdk (D17), and `flui` keeps `default = [\"material\"]`, the `flui::material` re-export and the Material half of the prelude. Probe B (flui→sdk + optional mat, mat→sdk) runs `cargo metadata` with exit 0, so there is no cycle.",
      "pros": [
        "Best newcomer DX: `cargo add flui` gives a styled app in one line. The README promise and today's template keep working.",
        "Nothing to change now: no examples, docs, doctests or facade-combos to move or rewrite.",
        "It is technically valid. The cycle argument against it is false once packages sit on flui-sdk."
      ],
      "cons": [
        "The Stable `flui` would publicly re-export an Evolving, independently versioned crate. Every Material major then becomes a `flui` major, which defeats the reason for splitting the packages out (D7, and Flutter 3.47's stated reason).",
        "Train coupling: `flui` cannot publish until a compatible `flui-material` is published from the separate packages workspace.",
        "The D17 gate needs an exemption ('except the facade, optionally'), which weakens the rule and invites more exemptions for Cupertino, devtools and hot reload.",
        "Users who pick Cupertino or a custom design system still get Material by default and must know to use `default-features = false`.",
        "The more crates.io consumers exist, the more expensive it becomes to remove the default later, and removing a default feature is a breaking change."
      ],
      "cost_now": "Near zero.",
      "cost_later": "High, and it grows with adoption. Each Material major forces a facade major. Removing the default later breaks every `cargo add flui` user at once.",
      "reversibility": "Low once `flui` is published as Stable: dropping a default feature and a public re-export is a SemVer break.",
      "fits_plan": "Poor. It contradicts D8 (the Stable facade), D7 (independent package cadence) and the D17 gate, and it would need the architecture doc amended."
    },
    {
      "id": "C",
      "name": "Hybrid: catalog-neutral `flui` (option A) plus a later optional batteries meta-crate (`flui-kit` = flui + flui-material)",
      "description": "Do option A now. Reserve a meta-crate name (for example `flui-kit`, or feature collections in Bevy's `default_app` style) that re-exports `flui` and `flui-material` for one-line installs. The meta-crate is Evolving: it pins both crates and bumps whenever either one bumps. Probe C (kit→{flui, mat}, mat→flui) is cycle-free. Build it only if the two-line install measurably hurts adoption.",
      "pros": [
        "It keeps every benefit of A for the Stable crate.",
        "It restores a one-line `cargo add` for people who want batteries, matching Bevy's batteries-included meta-crate, without putting Material into Stable.",
        "The volatility sits in a crate that is explicitly Evolving."
      ],
      "cons": [
        "It adds a third name for newcomers to understand (flui, flui-material, flui-kit), and docs.rs/README have to explain which one to pick.",
        "The meta-crate breaks whenever flui or flui-material bumps a major, which means frequent releases for a one-owner project.",
        "`flui create` already gives a one-command path, so its benefit over A is marginal."
      ],
      "cost_now": "Same as A if the meta-crate is deferred. If it is built now, add a small crate, its release automation and a docs section.",
      "cost_later": "Ongoing release maintenance for the meta-crate: a re-publish on every major of either dependency.",
      "reversibility": "High. A meta-crate can be deprecated without touching `flui`.",
      "fits_plan": "Compatible with D7/D8/D17. It puts a convenience layer on top and does not change core."
    },
    {
      "id": "D",
      "name": "Rename: the Stable neutral facade becomes `flui-core`, and `flui` becomes the batteries crate with Material",
      "description": "Swap the names so that the most discoverable name, `flui`, is the one-line batteries experience, and the catalog-neutral Stable surface lives under `flui-core`.",
      "pros": [
        "`cargo add flui` gives a styled app, and the name matches newcomer expectations.",
        "Library authors can depend on `flui-core` without Material."
      ],
      "cons": [
        "It inverts D8: the Stable promise sits on the less discoverable name, and the most prominent name becomes the Evolving, frequently breaking crate.",
        "Every doc, template, example and ADR that says `flui` has to be renamed, and flui-core becomes yet another 'core' in a workspace that already has foundation/tree/rendering.",
        "It has the same train and semver coupling problems as B on the `flui` name.",
        "Neither Flutter nor Dioxus does this, so there is no precedent for it."
      ],
      "cost_now": "High: a crate rename across the facade, CLI templates, docs and examples.",
      "cost_later": "Medium to high. The `flui` crate's version churns with every Material major.",
      "reversibility": "Low. Crate names on crates.io are effectively permanent once published.",
      "fits_plan": "Poor. It conflicts with D8 and with the facade's role as the Stable tier."
    }
  ],
  "recommended": "C",
  "rationale": "Recommend option A now and keep option C (a batteries meta-crate) in reserve; in practice that means shipping A. Evidence: D:\\flui\\Cargo.toml:525,598,605 show `material` is an optional dependency and a default feature today. crates/flui-cli/src/templates/counter.rs:22 writes a feature-less `flui` dependency, while :80/:115 use ThemeData/ElevatedButton through that default.\n\nThe architecture doc (:19, :684) gives the wrong reason for removing the feature. Probes A-D show that Cargo only reports a cycle when a package depends on the facade. Once Material depends on flui-sdk (D17), `default = [\"material\"]` compiles without a cycle (probe B, exit 0). The doc should cite the real reasons instead:\n1. A Stable `flui` that publicly re-exports an independently versioned Material turns every Material major into a facade major (Cargo SemVer reference on public dependencies).\n2. Publishing `flui` would have to wait for a `flui-material` release from the separate packages workspace.\n3. The D17 gate stays exemption-free.\n4. Flutter 3.47 moved Material and Cupertino out into material_ui/cupertino_ui for independent cadence, and Dioxus defaults to runtime only.\n\nOption B saves work today but gets more expensive to undo as adoption grows, and removing a default feature later is a SemVer break. Option D inverts D8.\n\nThe newcomer cost of A is covered by requirements in the same PR:\n- the counter template emits `flui-material` and its prelude import;\n- the README shows a two-line quick start;\n- docs.rs for `flui` gets a 'Design systems' section;\n- the Material showcase examples move to packages/flui-material/examples (a root dev-dependency also works, per probe D, but it would need a dev-dependency exemption in the D17 gate, so it is not preferred);\n- facade-combos is rewritten;\n- README.md:44-66, src/lib.rs:38-64/148-149/267-276 and docs/{FOUNDATIONS,getting-started,testing}.md are updated;\n- the change is announced publicly, as Slint did for its default-style change.\n\nBuild the `flui-kit` meta-crate only if there is evidence that the two-line install hurts adoption. It is additive and does not break anything later.\n\nUnverified: whether Flutter 3.47's `flutter create` adds material_ui automatically."
}
```

## judge_q6_facade_default_material_engineer

```json
{
  "choice": "A",
  "confidence": 0.82,
  "reasons": [
    "The code facts check out. D:\\flui\\Cargo.toml:598 has `default = [\"material\"]` and :605 has `material = [\"dep:flui-material\"]`. crates/flui-cli/src/templates/counter.rs:22 writes a `flui` dependency with no features (`source.dependency(\"flui\", &[])`), and :80/:115 use ThemeData/ElevatedButton. So the default is holding up both the template and the README promise.",
    "The architect's correction holds. The Cargo cycle only appears when a package depends on the facade (probe A). With Material on flui-sdk, a `material` default is valid (probe B). The case for A therefore rests on semver and release-train coupling, not on the cycle. Those reasons are sufficient on their own: `pub use flui_material as material` plus the prelude globs make Material a public dependency of the Stable `flui`, so every Material major forces a `flui` major.",
    "Train ordering is the practical blocker for B. With packages in a separate workspace (D7), publishing `flui` would wait on a `flui-material` release that itself depends on flui-sdk. That turns the independent cadence back into lockstep releases.",
    "A is cheap now and gets expensive later. Pre-1.0 there are no external consumers. Once `flui` is Stable, removing a default feature or a public re-export is a SemVer break, so B cannot be undone cheaply. A can later add an optional feature or a meta-crate without breaking anything.",
    "I pick A rather than C. C, as the architect framed it, is A plus a meta-crate that may never be built, so the decision is A. Reserving a name costs nothing, but committing to a third crate adds naming overhead and a release burden for a one-owner project, and `flui create` already gives newcomers a one-command start. D inverts D8 and still has B's coupling."
  ],
  "conditions": [
    "Atomic change: the facade feature removal and the counter template change (counter.rs:22 emits `flui-material = <ver>` and adds `use flui_material::prelude::*`) land in the same PR, plus a CLI test that generates the counter project and `cargo check`s it with the new facade, so a generated project cannot silently stop compiling.",
    "Update in the same PR: README.md:44-66 (two-line quick start), src/lib.rs:38-64 (crate docs and the material-gated doctest), src/lib.rs:148-149 and 267-276 (the re-export and the Material half of the prelude), the Cargo.toml comments at :592-597 and :667-677/:768, docs/FOUNDATIONS.md, docs/getting-started.md, docs/testing.md, and the AGENTS.md `[[example]] required-features` row.",
    "Rewrite `cargo xtask facade-combos` (tools/xtask/src/tasks/facade.rs) around the remaining features in that same PR, and keep a combo that builds the facade with `default = []`.",
    "Sequencing: Material must depend on flui-sdk/core crates, never on `flui`, before or together with this change. Enforce that with the D17 gate ('no core crate names an official package, even optionally'), implemented in `cargo xtask workspace`. State in the gate whether dev-dependencies are covered.",
    "Examples: Material showcases move to packages/flui-material/examples when the packages workspace exists. Until then, any interim root dev-dependency on flui-material must be an explicit, documented, temporary exemption in the gate, not a silent one.",
    "Correct the architecture doc (:19, :684). The rationale is public-dependency semver coupling, release-train ordering and gate simplicity. The cycle is a consequence only of a package depending on the facade, which D17 already forbids.",
    "Record the decision as an ADR, since it changes the cross-crate facade contract, and add a CHANGELOG entry announcing the Material-first default being dropped.",
    "Defer the meta-crate (`flui-kit`) until there is adoption evidence. If it is built, it is Evolving and re-exports only; it never adds API."
  ]
}
```

## judge_q6_facade_default_material_ecosystem_author

```json
{
  "choice": "A",
  "confidence": 0.82,
  "reasons": [
    "The facade is currently Material by default: D:\\flui\\Cargo.toml:525 declares flui-material as an optional dependency, :598 sets `default = [\"material\"]` and :605 defines `material = [\"dep:flui-material\"]`. That means the Stable `flui` (D8) would carry an Evolving, independently versioned crate as a public dependency. The Cargo SemVer reference says a public dependency's major bump is a major bump for the re-exporter. For an app author on H2-H4, `flui` would then churn with Material instead of being a steady base.",
    "A third-party package author needs a catalog-neutral base to build against. If `flui` pulls in Material by default, every package or app that depends on it without setting `default-features = false` compiles Material, and Cargo feature unification means one crate in the graph that forgets the flag turns it back on for everyone. Removing the feature eliminates that mistake entirely instead of relying on each author to remember the flag. This follows the 'make rules types, not reviews' stance.",
    "The architect's own probes change the rationale. Probe B shows `default=[\"material\"]` has no cycle once Material depends on flui-sdk, so the cycle argument in the architecture doc (:19, :684) is wrong. The decision still holds on three other grounds: semver coupling to a public dependency, release ordering (with `packages/` in a separate workspace, publishing `flui` would wait on a flui-material release), and a D17 gate with no exemptions.",
    "I pick A over C. The architect's recommended id is C, but the rationale it gives amounts to 'ship A, and build the meta-crate only if the data shows it is needed'. The decision to make now is A. A meta-crate is additive and needs no reserved decision; reserving the crates.io name costs nothing and is independent of this choice. Choosing C now invites building a third, frequently breaking crate for a one-owner project with no evidence it is needed.",
    "Doing this now is the cheapest it will ever be. The project is pre-1.0 with no external consumers (cargo metadata shows only flui-web-counter depends on flui). Dropping a default feature after a Stable publish would be a SemVer break for every `cargo add flui` user, so option B's cost grows with adoption.",
    "There is precedent. Flutter 3.47 moved Material and Cupertino into the standalone packages material_ui/cupertino_ui to give them their own release cadence, and Dioxus's facade defaults include no design system. Bevy's batteries-included defaults work only because everything it bundles ships in one release train, which is not true of FLUI's packages under D7.",
    "The DX cost is real but has contained fixes. The single-command path is `flui create`, which already needs editing: crates/flui-cli/src/templates/counter.rs:22 writes a `flui` dependency with no features, while :80/:115 use ThemeData and ElevatedButton, which only resolve through today's default."
  ],
  "conditions": [
    "The facade change and the counter template change must land in one atomic PR. The template must write `flui-material = <ver>` and `use flui_material::prelude::*;`, and its test_deps line (counter.rs:23) must be adjusted too. The PR must also add a CI test that generates the counter project and runs `cargo check` on it, so the template cannot silently break again.",
    "The architecture doc's rationale at :19 and :684 must be corrected: the cycle exists only when a package depends on the facade. The real reasons to record are public-dependency semver coupling, release-train ordering and the D17 gate. The ADR must say the same.",
    "The D17 gate must state its position on dev-dependencies explicitly. Preferred: move the Material showcase examples to packages/flui-material/examples and forbid core→official edges in the dev graph too. If any root dev-dependency on flui-material stays, the gate must name it as an exemption and give the reason.",
    "The same PR must update the docs: README.md:44-66 gets a two-line quick start plus `flui create`; in src/lib.rs, lines 38-64 (the doctest), 148-149 (the re-export) and 267-276 (the Material prelude) change; docs/FOUNDATIONS.md, docs/getting-started.md and docs/testing.md; the AGENTS.md examples row; and `cargo xtask facade-combos` is rewritten. The `flui` crate docs on docs.rs get a 'Design systems' section that points to flui-material and flui-cupertino.",
    "The CHANGELOG must carry a migration note: add `flui-material` and replace `flui::material::` with `flui_material::`. A `flui migrate` rewrite is optional, justified only if external users exist by then.",
    "No flui-kit meta-crate yet. Revisit only if install friction is documented in issues or user feedback.",
    "Do not make it worse on a partial step: removing the `material` feature from `flui` is acceptable only once flui-material can be depended on directly from outside the repo, meaning it builds from a path or git source in a fresh project outside the workspace. Otherwise H0-H1 app authors lose the only way they have to get Material."
  ]
}
```

## judge_q6_facade_default_material_owner

```json
{
  "choice": "A",
  "confidence": 0.82,
  "reasons": [
    "The evidence checks out. D:\\flui\\Cargo.toml:525 makes flui-material an optional dependency, :598 sets `default = [\"material\"]` and :605 defines `material = [\"dep:flui-material\"]`. crates/flui-cli/src/templates/counter.rs:22 writes a `flui` dependency with no features, yet :80 uses `ThemeData` and :115 uses `ElevatedButton`. So the template relies on the default today.",
    "The architect's option C amounts to shipping A now and adding a meta-crate later if it is needed. I pick A because it is the concrete decision to make. A meta-crate can be added later without breaking anything, so C adds no work now. It would only add a third crate name and a release burden that a sole owner with AI contributors should not take on before there is evidence adoption needs it.",
    "The cycle is not the real reason to drop the feature. Probe B shows a `material` default compiles once Material depends on flui-sdk. The deciding reasons are these. A Stable facade that publicly re-exports an Evolving package turns every Material major into a `flui` major (Cargo SemVer rules on public dependencies). Publishing `flui` would have to wait for a `flui-material` release. The D17 gate 'core never names an official package, even optionally' stays free of exemptions.",
    "Reversibility favours A. Adding an optional feature back later is additive and non-breaking. Removing a default feature after `flui` is published as Stable breaks every user of a plain `cargo add flui`. The project is pre-1.0 and has no external consumers, so this is the cheapest time to make the change.",
    "Precedent supports it. Flutter 3.47 moved Material and Cupertino into the separate packages material_ui and cupertino_ui so they could release on their own cadence. Dioxus's default features include no design system. Bevy's batteries-included defaults work only because everything it pulls in ships in one release train with the meta-crate, and D7 deliberately breaks that for FLUI.",
    "Option B costs almost nothing today but gets more expensive to reverse as adoption grows, and it contradicts D7, D8 and D17. Option D gives up the most discoverable name to a volatile crate, and a published crate name effectively cannot be changed."
  ],
  "conditions": [
    "Land it as one atomic PR, or a pair where the template goes first. The counter template (crates/flui-cli/src/templates/counter.rs:22) must emit `flui-material = <ver>` and `use flui_material::prelude::*;` in the same change that removes the default. Add a test in flui-cli that builds or type-checks the generated counter project with the facade's default features, so it fails if the template ever depends on a facade feature again.",
    "Keep Material in the default `flui create` template so the first run looks the same as today.",
    "Update in the same PR: README.md:44-66 (a two-line quick start), src/lib.rs:38-64 (docs and doctest), src/lib.rs:148-149 and :267-276 (the re-export and the Material half of the prelude), a 'Design systems' section in the docs.rs crate docs, docs/FOUNDATIONS.md, docs/getting-started.md, docs/testing.md, and the AGENTS.md row for `[[example]] required-features`.",
    "Rewrite `cargo xtask facade-combos` so it no longer references material or cupertino, and add or extend a `cargo xtask workspace` check that enforces the D17 rule 'no core crate names an official package, even optionally'. That check should also cover dev-dependencies, or it should name an explicit exemption for them.",
    "Move the Material showcase examples to packages/flui-material/examples. Keep the catalog-free examples at the root. Do not keep a root dev-dependency on flui-material unless the gate records that exemption.",
    "Correct the architecture doc (flui-global-architecture.md:19 and :684): the reason is semver and public-dependency coupling, release-train ordering and the gate, not a Cargo cycle. Cite probes A and B. Record the decision in an ADR, because the facade surface is a cross-crate contract.",
    "Ship an announcement or CHANGELOG entry with the change, plus a migration note (or a `flui migrate` step) that rewrites `flui::material::*` imports to `flui_material::*`.",
    "Defer the batteries meta-crate (option C). Reconsider it only if there is evidence that the two-line install hurts adoption, and if it is built, mark it Evolving."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "Option A claims packages 'ship on their own cadence' and that Material stops coupling to the facade. That independence is false under the architecture's own pinning rules. The facade pins flui-sdk exactly, and train crates pin internal edges exactly. So once a core train release is published, even a patch, the newest `flui` and the already-published `flui-material` cannot resolve together until Material is re-released. Under A the user, not the facade, has to find a matching pair. `cargo add flui` followed by `cargo add flui-material` fails with a resolver error whenever the train has moved ahead of the package. Option B hides this because the facade's optional dependency selects a compatible Material.",
      "evidence": "flui-global-architecture.md:176 says internal edges get exact pins. :209 says the facade pins an exact flui-sdk version. :177 says packages depend on flui-sdk by version, and CI builds `packages/` only against the last published train. Probe scratchpad/probe-q6/E: `flui` pins anstyle =1.0.14 (standing in for a train patch) and `mat` pins anstyle =1.0.13 (the older train). An app depending on both runs `cargo metadata --offline`, which prints 'failed to select a version for `anstyle` ... all possible versions conflict with previously selected packages' and exits 101.",
      "severity": "major",
      "fix": "Keep A, but add these to its conditions. (1) flui-sdk and packages use caret requirements, not `=`, on shared internal crates within a compatible range, or else flui-sdk becomes the single crate both sides depend on with a caret requirement. (2) Add a CI job that resolves and builds the latest published `flui` with the latest `flui-material`, beyond the doc :177 check against the last train. (3) Have `flui create` and the README pin a known-compatible pair, and publish a compatibility table. (4) Remove 'independent cadence' from A's list of pros, or qualify it."
    },
    {
      "problem": "The same-PR update list misses a CI workflow line that uses the feature being removed. Once `material` is dropped, the heavy CI job fails with an unknown-feature error. AGENTS.md says `.github/workflows/` is left alone unless the task is about it, so the PR has to state explicitly that it touches workflows.",
      "evidence": ".github/workflows/ci.yml:924 `cargo build -p flui --features material --example sliver_demo --locked`. The example is declared with required-features material in Cargo.toml (the lines around :700-738), and it uses `flui::material` (grep: examples/sliver_demo.rs).",
      "severity": "major",
      "fix": "Add ci.yml:924 (and the comment at :1012-1019 about `--no-default-features` keeping Material out) to the atomic PR. Give the PR the `full-ci` label so the heavy job runs before merge."
    },
    {
      "problem": "The template condition in every judge's list imports `flui_material::prelude::*`, but flui-material has no prelude module. The template as specified would not compile.",
      "evidence": "`Select-String crates/flui-material/src/lib.rs -Pattern prelude` returns nothing. The crate root only has flat `pub use` lines (lib.rs:110-157). The facade's current Material prelude is a hand-picked list (src/lib.rs:267-276).",
      "severity": "minor",
      "fix": "Add `flui_material::prelude` in the same PR, containing the set listed at src/lib.rs:267-276 and nothing that re-exports flui_widgets names, to avoid glob ambiguity with `flui::prelude::*`. Alternatively, have the template import the named Material types explicitly. Pin the resulting glob pair with a doctest or snapshot."
    },
    {
      "problem": "The list of docs to update is incomplete. Besides README, lib.rs and the three docs files, other hand-written docs and tests use `flui::material` or `flui-material` through the facade, and they would go stale or fail.",
      "evidence": "Grep for `flui::(material|cupertino)` finds ARCHITECTURE.md (1 hit), book/src/getting-started/tutorial-todo.md (1), tests/facade_smoke.rs (14) and examples/{workload_probe,lifecycle_probe,todo,sliver_demo}.rs. Grep for material in book/ finds book/src/{mapping,cookbook/themes,cookbook/async,cookbook/forms,widgets/catalog}.md. There are also `[[test]]` entries with required-features material or cupertino at Cargo.toml:741-773.",
      "severity": "minor",
      "fix": "Generate the list for the PR with `rg 'flui::(material|cupertino)|features.*(material|cupertino)'` over the whole repo, excluding docs/archive, instead of the hand-written list. Include book/, ARCHITECTURE.md, tests/facade_smoke.rs and the `[[test]]` entries."
    }
  ]
}
```
