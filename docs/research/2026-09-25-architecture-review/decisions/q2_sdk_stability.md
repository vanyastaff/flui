# Decision panel: q2_sdk_stability

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "The architecture doc already proposes flui-sdk as a separate, host-free Evolving crate (flui-global-architecture.md:14, :169, :209). It also re-exports it from the Stable facade with `pub use flui_sdk as sdk; // Evolving, own version; facade pins it exactly` (:204), and asks the owner about it in §14 question 2 (:724). The re-export at :204 contradicts the claim at :209 that sdk is outside the Stable promise of `flui`. See the constraints.",
    "The workspace ships one version train today: `[workspace.package] version = \"0.2.0-dev\"` (Cargo.toml:105-106), and 27 manifests use `version.workspace = true` (grep of crates/*/Cargo.toml). Internal edges are already exact pins: `grep -rhoE 'version = \"=0\\.2\\.0-dev\"' crates/*/Cargo.toml Cargo.toml | wc -l` returns 172. flui-material alone has 14.",
    "Package crates today depend directly on internal crates. flui-material: widgets, view, types, objects, rendering, foundation, animation, interaction, scheduler (9 crates). flui-cupertino: view, widgets, objects, foundation, types, animation. flui-devtools: foundation, scheduler. flui-hot-reload: layer, foundation, view, rendering. Source: `[dependencies]` of each crates/<c>/Cargo.toml.",
    "Import counts from `grep -rhoE 'flui_[a-z_]+::' crates/flui-material/src crates/flui-cupertino/src`: widgets 180, types 180, view 106, foundation 29, rendering 26, painting 9, animation 9, objects 4, interaction 3, scheduler 2, semantics 1.",
    "Most of the non-catalog items the packages import would belong to the planned Stable facade modules anyway (rendering/animation/interaction/foundation): BoxConstraints (9), Listenable/ListenerId/ChangeNotifier/ValueNotifier, Curve/AnimationController/Curves, FocusNode, HitTestBehavior, BoxProtocol. The genuinely internal remainder is about 12 items: flui_painting::DrawOp (9 uses), flui_rendering::pipeline::Canvas (4), RenderUpdateImpact (9), flui_scheduler::LocalPostFrameHandle (2), FrameSnapshot, InputEpochId, flui_objects::{RenderPhysicalShape, RenderTable, TranslationFraction}, flui_foundation::{ElementId::new, RebuildReason, observe::*}. The last three are used by devtools and hot-reload. Command: grep -rhoE 'flui_(rendering|painting|objects|interaction|scheduler|semantics|animation|foundation)::[A-Za-z_:]+' over material/cupertino/devtools/hot-reload src | sort | uniq -c.",
    "Material and Cupertino use no doc(hidden) or runtime-internals paths: grep for `runtime_internals|__\\w*::|internals::|doc(hidden)` in crates/flui-material/src and crates/flui-cupertino/src returns nothing. The `runtime-internals` feature (crates/flui-view/Cargo.toml:118) is enabled only by flui-app (:90), flui-hot-reload (:27) and flui-testing (:51).",
    "Dependency closure after deduplicating by name+version (`cargo tree -p X -e normal --prefix none | sed 's/ (\\*)//' | awk '{print $1\" \"$2}' | sort -u | wc -l`): flui-material 127, flui 191. This matches the doc's claim at :14. `cargo tree -p flui-material -e normal -i wgpu` fails with `did not match any packages`, so material does not reach wgpu today. It does reach flui-platform through flui-interaction.",
    "The facade today re-exports whole internal crates (`pub use flui_animation as animation; pub use flui_app as app; ... pub use flui_view as view; pub use flui_widgets as widgets;` at src/lib.rs:126-152) and has `default = [\"material\"]` plus `material`/`cupertino`/`localizations` features (Cargo.toml:598-615).",
    "No semver tooling exists in the repo yet: `grep -rn semver tools/xtask/src .github/workflows` matches only a comment in .github/workflows/weekly.yml:5 (latest-deps).",
    "Sibling crates in our own lockfile use both versioning models. wgpu, wgpu-core, wgpu-hal, wgpu-types and naga move in lockstep (all 30.0.1). accesskit (0.25.0) and accesskit_consumer (0.39.0) are numbered independently, but the consumer must re-release whenever accesskit breaks. Source: Cargo.lock grep."
  ],
  "market_precedents": [
    {
      "who": "Flutter (framework compatibility policy)",
      "what": "There is no separate SDK or package-author layer. Everything in package:flutter (rendering, widgets, gestures) is covered by the same empirical rule: a change is breaking if it fails a test in the flutter/tests registry, and deprecation or removal follows the breaking-change process.",
      "outcome_or_lesson": "Flutter keeps a single promise and defines it by tests from consumers, not by layer. The applicable lesson for FLUI is the test registry: official packages built in CI against the published train (D7) do the same job. Flutter's framework surface was mostly public from day one, so it never needed an SDK tier. FLUI has about 12 truly internal items that packages need, which is the argument for a small Evolving tier rather than a whole crate of re-exports.",
      "source": "https://docs.flutter.dev/resources/compatibility"
    },
    {
      "who": "Flutter material_ui / cupertino_ui packages (3.47, Aug 2026)",
      "what": "Material and Cupertino moved to standalone pub packages that import the framework's ordinary public API. No new package-author SDK layer was announced. The stated goal was a release cadence independent of the SDK: issue #163400 complains of about 3.1 months between stable releases and of cherry-picks blocked by internal dependencies.",
      "outcome_or_lesson": "material_ui shipped 1.0.0, 1.0.1, 1.1.0, 1.1.1, 1.2.0 and 1.4.0 within about 44 days, and retracted 1.3.0. A design-system package needs its own fast cadence, so whatever it depends on must change more slowly than it does. An sdk that breaks every train pushes the package back onto the framework's cadence.",
      "source": "https://pub.dev/packages/material_ui/versions ; https://github.com/flutter/flutter/issues/163400 ; https://docs.flutter.dev/release/breaking-changes/material-ui-and-cupertino-ui"
    },
    {
      "who": "Bevy plugin ecosystem",
      "what": "Bevy has no stable plugin API. Every 0.x release ships a migration guide, and third-party plugins keep compatibility tables. bevy_egui's table has 14 rows, and each Bevy release needed a new bevy_egui version (0.17 -> 0.37-0.38, 0.18 -> 0.39, 0.19 -> 0.40-0.42).",
      "outcome_or_lesson": "This is what 'packages depend on internals with exact pins' or 'sdk Evolving forever' looks like at scale: each train fragments the ecosystem until plugins catch up. An Evolving sdk is acceptable only with a narrow surface, breaks limited to train boundaries, and a graduation path into Stable.",
      "source": "https://github.com/vladbat00/bevy_egui ; https://bevy.org/learn/migration-guides/introduction/"
    },
    {
      "who": "cargo-semver-checks",
      "what": "The tool analyzes one crate's rustdoc JSON at a time. Its maintainer calls cross-crate re-exports the top source of false positives (tracking issue #638): rustdoc does not inline foreign re-exports, and the tool cannot tell which crate version a foreign item came from. Its README also says it does not catch type changes of fields or parameters, or generics and lifetime breaks.",
      "outcome_or_lesson": "A facade or sdk made mostly of `pub use internal_crate::X` is the case semver-checks handles worst. Measuring the Stable closure with cargo-public-api, and treating sdk checks as advisory, fits this. Putting `pub use flui_sdk as sdk` in the Stable facade would pull sdk churn into what semver-checks reports for `flui`, and no module-level exemption is known (hypothesis, not verified).",
      "source": "https://github.com/obi1kenobi/cargo-semver-checks/issues/638 ; https://predr.ag/blog/four-challenges-cargo-semver-checks-has-yet-to-tackle/ ; https://github.com/obi1kenobi/cargo-semver-checks"
    },
    {
      "who": "Tokio",
      "what": "Unstable APIs are gated by `--cfg tokio_unstable` in RUSTFLAGS instead of a Cargo feature. They are exempt from semver in 1.x, and the docs say Cargo has no real opt-in mechanism.",
      "outcome_or_lesson": "If an Evolving area lives inside the Stable crate, a Cargo feature gate leaks through feature unification. Probe 2 reproduces the leak. A cfg flag set by the final binary is the working Rust opt-in model.",
      "source": "https://docs.rs/tokio/latest/tokio/#unstable-features"
    },
    {
      "who": "Kotlin / Jetpack Compose (@RequiresOptIn, @ExperimentalFoundationApi)",
      "what": "Evolving APIs live inside stable artifacts but carry an opt-in marker. A caller whose signature mentions a marked declaration must opt in or propagate the marker, and APIs graduate to stable by dropping the marker.",
      "outcome_or_lesson": "Graduation per item, not per crate, is the path the sdk needs after H3: move items one by one into Stable `flui` modules. Rust has no compiler-enforced opt-in propagation, so the separate crate plus exact pin is the nearest equivalent that a type checker enforces.",
      "source": "https://kotlinlang.org/docs/opt-in-requirements.html"
    },
    {
      "who": "Linebender Masonry / Xilem",
      "what": "Masonry is a separately published crate whose API is 'geared towards creating GUI libraries', and Xilem is the app-facing layer on top. Both are pre-1.0 and released together.",
      "outcome_or_lesson": "This is a real Rust precedent for splitting library authors from app authors into distinct crates. It is also released in lockstep, so a separate crate did not mean an independent cadence.",
      "source": "https://docs.rs/masonry/latest/masonry/ ; https://github.com/linebender/xilem"
    },
    {
      "who": "wgpu family / AccessKit adapters (from our own Cargo.lock)",
      "what": "wgpu-core, wgpu-hal and wgpu-types share the wgpu version (30.0.1). accesskit_consumer is numbered independently (0.39 against accesskit 0.25) but re-releases whenever accesskit breaks.",
      "outcome_or_lesson": "A layer that re-exports another crate's types cannot have a truly independent version. Its number is either lockstep or a separate counter that bumps on every upstream break, which fits flui-sdk as `0.N` bumped per train.",
      "source": "D:/flui/Cargo.lock (grep of name/version entries)"
    }
  ],
  "constraints": [
    "Probe 1 (type identity): an sdk version cannot be independent of the train. flui-sdk will re-export types (BoxConstraints, DrawOp, Canvas) from the same internal crates as the facade. If a package's flui-sdk and the app's `flui` resolve to different internal-crate versions, the values are different types (E0308 in Probe 1). With exact internal pins (172 today), the more likely result is a resolver failure: Cargo does not allow two semver-compatible versions of one package (hypothesis for the `=` pin case; not reproduced, since it needs a registry). So each flui-sdk release must be tied to exactly one train. It can have its own number, but in practice it moves in lockstep, like wgpu-core, or bumps on every train, like accesskit_consumer.",
    "Recommendation (answer to §14 q2): yes to a separate crate `flui-sdk` outside the Stable promise of `flui` until H3, and after H3 by default. Version it as `0.N` and bump N on every train that changes it, or bump on every train unconditionally, which is simpler for one author. Its manifest pins the internal crates exactly, and the facade pins it exactly. A package's `flui-sdk = \"0.N\"` requirement then selects its train, and a mismatch shows up as a resolver error at `cargo add` time rather than an E0308 deep in a build.",
    "Remove `pub use flui_sdk as sdk` from the Stable facade (doc line :204). Apps do not need the sdk. Re-exporting it makes Evolving items part of `flui`'s public API: users reach them through `flui::sdk::*`, and semver-checks on `flui` would report sdk churn as `flui` breaks (the cross-crate re-export false-positive class, #638). If devtools or templates need one path, use `#[doc(hidden)] pub use` or nothing.",
    "Do not gate an Evolving area of the facade with a Cargo feature. Probe 2 shows Cargo feature unification lets an app call `f2::sdk::draw_op()` without requesting `unstable`, because one package enabled it. This rules out the 'Evolving module inside facade behind `unstable`' alternative as an opt-in mechanism. If an in-facade experimental area is wanted, use a tokio-style `--cfg flui_unstable`.",
    "'Part of the Stable facade' is rejected on two counts. It freezes about 12 internal items at H3 before a second implementation has proven them: DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle, FrameSnapshot, InputEpochId, RenderPhysicalShape, RenderTable, TranslationFraction, ElementId::new, RebuildReason, observe. And it makes every package pay for the host: 191 crates against 127 for material today.",
    "'No sdk, packages pin internal crates exactly' is today's state (material has 14 `=0.2.0-dev` pins across 9 internal crates). It encodes internal topology into every package manifest. The migration already plans topology changes: delete flui-tree and flui-localizations, add flui-reactive and flui-runtime. Each one would break every external package manifest, not just its code, which is the Bevy pattern at the manifest level. The sdk exists to absorb topology changes.",
    "Keep the sdk narrow and layered. Most of what packages import (BoxConstraints, Listenable/ChangeNotifier, Curve/AnimationController, FocusNode, HitTestBehavior, and the view and widgets catalog types) belongs to Stable modules anyway. The sdk should re-export those under the same module paths as the Stable facade: identical type identity, and stable in practice because the Stable facade constrains the defining crates. The truly Evolving items then sit in named sdk modules (`flui_sdk::paint`, `::pipeline`, `::hooks`, `::gpu`), so a package's exposure to Evolving can be counted by grep. Hypothesis: the Evolving part stays around 12 to 20 items plus the hooks (RealmObserver, InspectHook, DevReloadHook); measure with cargo-public-api at W3.",
    "Graduation policy after H3, following the Kotlin opt-in precedent: an Evolving sdk item graduates into a Stable `flui` module once it has survived N trains unchanged and has a second implementer or consumer. The sdk then keeps a pure re-export under its old path. 'Evolving forever' should apply only to `flui_sdk::gpu` (wgpu interop, which already re-pins the wgpu major each train, doc :314) and the dev hooks.",
    "Verification path matching Flutter's test registry: CI builds `packages/` (Material, Cupertino, devtools) against the last published train (D7). That catches sdk breaks empirically, which matters because semver-checks cannot see type changes or cross-crate re-exports. cargo-semver-checks on flui-sdk stays advisory.",
    "Cadence risk, taken from material_ui's first 44 days (six releases, one retraction): if the sdk breaks every train, official packages cannot release faster than the train in a way that matters. Mitigation: the sdk only breaks at train boundaries, and each break ships with a migration note in the train changelog, as Bevy's per-release guides do.",
    "Not verified: crates.io availability of the name `flui-sdk` (the crates.io MCP server failed to connect this session); whether cargo-semver-checks has a per-module exemption (believed not); the exact resolver error text for conflicting `=` pins through a registry (only the path-dependency E0308 variant was reproduced)."
  ],
  "experiments_run": [
    "Probe 1, type identity (C:/Users/vanya/AppData/Local/Temp/claude/D--flui/bbb28042-f972-4a10-8e94-731819e26161/scratchpad/probe-sdk/{core1,core2,sdk,facade,pkg}): sdk re-exports core 0.1's BoxConstraints, facade re-exports core 0.2's, and pkg passes `facade::constraints()` to `sdk::paint_shape`. `cargo check` fails: `error[E0308]: mismatched types ... expected sdk::BoxConstraints, found facade::rendering::BoxConstraints` and `note: there are multiple different versions of crate core in the dependency graph`. Conclusion: sdk must be version-locked to the train.",
    "Probe 2, feature leak (probe-sdk/{f2,pk2,app2}): f2 has `#[cfg(feature=\"unstable\")] pub mod sdk`, pk2 enables `f2/unstable`, and app2 depends on f2 without the feature but calls `f2::sdk::draw_op()`. `cargo check` passes (`Finished dev profile`). Conclusion: a Cargo-feature-gated Evolving module in the facade leaks to apps through feature unification.",
    "`cargo tree -p flui-material|flui -e normal --prefix none`, deduplicated by name+version: 127 against 191 crates. `cargo tree -p flui-material -e normal -i wgpu` reports that wgpu is not in material's graph.",
    "grep inventory of every `flui_*::path` used by crates/flui-{material,cupertino,devtools,hot-reload}/src, to split what already belongs to Stable modules from about 12 truly internal items.",
    "`grep -rhoE 'version = \"=0\\.2\\.0-dev\"' crates/*/Cargo.toml Cargo.toml | wc -l` returns 172 (14 in flui-material). `grep version Cargo.lock` for wgpu*/naga (all 30.0.1) and accesskit 0.25.0 against accesskit_consumer 0.39.0."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "Separate train-locked Evolving crate `flui-sdk`, not re-exported by the facade (hybrid: Stable paths plus named Evolving modules)",
      "description": "`flui-sdk` is its own crate in tier K. It is host-free (no flui-app, engine or wgpu) and exactly pins the internal crates of one train. It is numbered `0.N` and never reaches 1.0 before its Evolving items graduate. It has two parts. (1) Re-exports of Stable-closure items under the same module paths as the `flui` facade (view, widgets, rendering, animation, interaction, foundation types such as BoxConstraints, ChangeNotifier, Curve, FocusNode), which gives identical type identity and practical stability. (2) Named Evolving modules `flui_sdk::{paint, pipeline, hooks, gpu}` for the roughly 12 genuinely internal items (DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle, FrameSnapshot, InputEpochId, RenderPhysicalShape, RenderTable, TranslationFraction, ElementId::new, RebuildReason, observe) plus Surface, post-frame, tokens, RealmObserver, InspectHook and DevReloadHook. The facade does not `pub use flui_sdk`: delete flui-global-architecture.md:204, which contradicts :209. Breaks happen only at train boundaries and each one ships a migration note. After H3, an item graduates into a Stable `flui` module once it has gone N trains unchanged and has a second consumer, and the sdk keeps a pure re-export at the old path.",
      "pros": [
        "Official packages build without the host: 127 crates for flui-material today against 191 for `flui` (cargo tree, deduplicated). This matches decision 2 of the doc (:14).",
        "Packages stop encoding internal topology in their manifests. The planned deletion of flui-tree and flui-localizations and the addition of flui-reactive and flui-runtime change only the sdk's manifest, not those of Material, Cupertino or third-party packages. Today material carries 14 `=0.2.0-dev` pins across 9 internal crates.",
        "The Stable promise of `flui` excludes the 12 unproven internals, so H3 does not freeze DrawOp or Canvas before a second implementation (engine-cpu) has exercised them.",
        "A package's exposure to Evolving is countable: grep `flui_sdk::(paint|pipeline|hooks|gpu)` in the package.",
        "A train mismatch surfaces as a resolver error on the `flui-sdk = \"0.N\"` requirement, not as an E0308 deep in the build (Probe 1 reproduced the E0308 variant with path dependencies).",
        "It follows real precedents: Masonry is a separate library-author crate, and accesskit_consumer is numbered independently but re-releases on upstream breaks.",
        "Easy to verify empirically: CI builds packages/ against the last published train (D7), which plays the role of Flutter's tests registry."
      ],
      "cons": [
        "One more published crate and one more version counter to maintain. Every patch of the train needs a matching sdk patch (0.N.k) because the internal pins are exact. That holds from the exact-pin behavior, but the registry resolver-error text is not reproduced.",
        "The version is not really independent (Probe 1): in practice it is lockstep with the train, so its own number is mainly a semver shield after `flui` reaches 1.x.",
        "cargo-semver-checks handles re-export-heavy crates poorly (issue #638), so the sdk check stays advisory and correctness rests on building the packages in CI.",
        "It adds a Bevy-like migration burden for third-party package authors on every train that touches an Evolving module, until the items graduate.",
        "Two import roots for package authors (flui_sdk in the package, flui in the app). Same types, but the docs have to explain it."
      ],
      "cost_now": "Low to medium. Create the crate at W3 when the tiers land. Move about 14 manifest pins per package onto one `flui-sdk` pin. Rewrite about 550 `flui_*::` imports in material and cupertino, which is mechanical because most are widgets, types or view paths that keep their shape. Add a `tier-kind = evolving` check to `cargo xtask workspace`. Run one cargo-public-api spike to measure the Evolving closure (hypothesis: 12 to 20 items plus the hooks).",
      "cost_later": "Low. Graduation is per item and additive. Each train costs one sdk release, automated in the same publish run, plus a changelog migration section when an Evolving module changes. The sdk absorbs topology refactors instead of every package.",
      "reversibility": "High. Folding the sdk into the facade later (option B) is additive: publish the same items in Stable modules and leave flui-sdk as a deprecated re-export shell. Going the other way, out of a Stable promise, is not reversible after 1.0.",
      "fits_plan": "Matches the doc's decisions (:14, :169, :209) and D7 (packages built against the published train). It needs one correction: drop `pub use flui_sdk as sdk` at :204, because it would pull sdk churn into the public API of `flui` and into semver-checks. It also answers §14 q2 (:724): yes, outside the Stable promise until H3, and outside it afterwards for the parts that have not graduated. `gpu` and the dev hooks stay Evolving indefinitely, since gpu re-pins the wgpu major each train (:314)."
    },
    {
      "id": "B",
      "name": "Package-author surface is part of the Stable `flui` facade",
      "description": "No sdk crate. Everything packages need, including DrawOp, Canvas, the hooks and RenderPhysicalShape, gets Stable modules in `flui`, and packages depend on `flui` like apps do. This is the closest analogue to Flutter, where package:flutter carries one promise for all layers.",
      "pros": [
        "One crate, one promise, one version: the simplest mental model and the closest to Flutter's compatibility policy.",
        "No type-identity questions between two roots.",
        "semver-checks covers everything with one tool run (still subject to the re-export false positives)."
      ],
      "cons": [
        "It freezes about 12 unproven internals at H3, including DrawOp, Canvas and RenderUpdateImpact, which the engine-cpu and flui-runtime work is still reshaping. The result is a 2.0 soon after 1.0, or workarounds around a bad shape, which AGENTS.md explicitly rejects.",
        "Every package pays for the host: 191 crates against 127, pulling flui-app, engine, wgpu and naga into Material's build (doc :14).",
        "It creates a feature cycle with the facade's optional material and cupertino features (doc point 7).",
        "Flutter could do this only because its framework surface was public from day one. FLUI's needed internals are not yet designed for publication."
      ],
      "cost_now": "Medium to high. Every internal item packages touch has to be designed to Stable quality before H3, and the host has to be split out of the facade some other way to fix build cost.",
      "cost_later": "High. Every mistake in the frozen internals needs a major bump of `flui`, which breaks all apps and not only package authors.",
      "reversibility": "Low after 1.0. Removing items from a Stable promise is a major release.",
      "fits_plan": "Conflicts with decision 2 (:14) that packages avoid the facade, with the tier model, and with the build-cost goal. Not recommended."
    },
    {
      "id": "C",
      "name": "Evolving module inside the facade (`flui::sdk` / `flui::unstable`) behind an opt-in",
      "description": "Keep one crate. Put the package-author internals in an Evolving module of `flui`, exempt from its semver promise, opted into either with a Cargo feature `unstable` or tokio-style with `--cfg flui_unstable` in RUSTFLAGS.",
      "pros": [
        "One crate to publish and version, with no second counter.",
        "The Kotlin @RequiresOptIn and tokio_unstable precedents make per-item graduation natural.",
        "With `--cfg flui_unstable` the final binary really controls the opt-in."
      ],
      "cons": [
        "A Cargo feature gate does not work as an opt-in. Probe 2 shows app2 calls `f2::sdk::draw_op()` without requesting the feature, because a package enabled it through feature unification.",
        "A cfg gate means every consumer of Material must set RUSTFLAGS, which is unacceptable for the default design-system package and awkward for `cargo add`.",
        "Packages still depend on the whole facade: the 191-crate build and the facade feature cycle remain.",
        "semver-checks on `flui` reports Evolving churn as `flui` breaks. No per-module exemption is known (hypothesis, unverified), so the gate becomes noisy or gets disabled.",
        "The Evolving status is only a doc promise inside a crate that users read as Stable."
      ],
      "cost_now": "Low: a module plus a cfg or feature.",
      "cost_later": "Medium to high. Semver noise on every train, confused users who reach `flui::sdk::*` from apps, and the host build cost for every package.",
      "reversibility": "Medium. The module can be split out into option A later, but import paths in every package change.",
      "fits_plan": "Contradicts decision 2 (packages do not depend on the facade). The doc's `#[cfg(feature = \"unstable\")] pub mod unstable` (:205) has the same leak and should become a `--cfg flui_unstable` gate or be removed. That is a separate note, and it matters for any in-facade experimental area. Not recommended for the package surface."
    },
    {
      "id": "D",
      "name": "No sdk: packages depend on internal crates with exact pins (status quo)",
      "description": "Material, Cupertino, devtools and third-party packages keep depending on flui-widgets, flui-view, flui-rendering and the other internal crates directly, each with `=train` pins (172 exact pins in the workspace today, 14 in material). Internal crates carry no promise.",
      "pros": [
        "Zero work now; this is how the repo builds today.",
        "Minimal build closure per package: each depends only on what it uses.",
        "No second surface to design or document."
      ],
      "cons": [
        "It encodes internal topology into every package manifest. The planned crate deletions and additions (flui-tree, flui-localizations, flui-reactive, flui-runtime) break every external package manifest as well as its code, which is Bevy's pattern at the manifest level (bevy_egui needed a new release for every Bevy release across 14 table rows).",
        "The 'internal' tier-kind stops meaning anything once third parties depend on it, so the tier gate cannot tell a public from a private crate.",
        "No place to hang a graduation policy or count a package's exposure to unstable items.",
        "Third-party authors have to learn 9 or more crate names and their pin discipline."
      ],
      "cost_now": "None.",
      "cost_later": "High, and it grows with every third-party package. Every topology refactor becomes an ecosystem-wide break, and the fix (introducing an sdk) gets more expensive once external manifests exist.",
      "reversibility": "Medium now, and low once external packages exist. Moving to A later means every package rewrites its manifest and imports, which is exactly the cost A pays once, now, with only first-party packages affected.",
      "fits_plan": "Acceptable only as the interim state until W3, when the tiers and flui-sdk land. It contradicts the migration's topology changes and the tier-kind check. Not recommended as the target."
    }
  ],
  "recommended": "A",
  "rationale": "I recommend option A: make `flui-sdk` a separate, host-free crate, pinned to one train and numbered `0.N`, and keep it outside the Stable promise of `flui` until H3. After H3 it stays outside that promise, except for items that graduate one at a time.\n\n1. **The version cannot really be independent.** In Probe 1, two different internal-crate versions under the sdk and the facade give `error[E0308]: mismatched types ... multiple different versions of crate core`. So the sdk moves with the train, like wgpu-core or accesskit_consumer. Its own `0.N` number is mainly a semver shield for when `flui` is 1.x. Each train, including patch releases, publishes a matching sdk.\n2. **Build cost.** Packages must not pay for the host: 127 crates for flui-material today against 191 for `flui` (cargo tree, deduplicated). That rules out B and C.\n3. **Status quo.** D, packages pinning internal crates exactly, is today's state. It turns each planned topology change (deleting flui-tree and flui-localizations, adding flui-reactive and flui-runtime) into a break of every package manifest. That is Bevy's plugin churn, and fixing it after third-party packages exist costs more than fixing it now, while only first-party packages are affected.\n4. **Why not freeze it all at H3.** Only about 12 items are genuinely internal: DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle, FrameSnapshot, InputEpochId, RenderPhysicalShape, RenderTable, TranslationFraction, ElementId::new, RebuildReason and observe (grep inventory of material, cupertino, devtools and hot-reload). Freezing them in the Stable facade at H3, before engine-cpu and flui-runtime have exercised them, is how you end up shipping 2.0 soon after 1.0.\n\n**Two corrections to the architecture doc:**\n- **Delete `pub use flui_sdk as sdk` at flui-global-architecture.md:204.** It contradicts :209, exposes Evolving items to apps through `flui::sdk::*`, and makes cargo-semver-checks on `flui` report sdk churn as breaks of `flui`. That is the cross-crate re-export false-positive class in cargo-semver-checks issue #638.\n- **Replace `#[cfg(feature = \"unstable\")] pub mod unstable` (:205)** with a `--cfg flui_unstable` gate, or remove it. Probe 2 showed that a Cargo feature does not work as an opt-in: feature unification let an app call the gated module without requesting the feature.\n\n**How to shape the sdk:**\n- **Two parts.** Stable-closure types are re-exported under the same module paths as the facade, which gives the same type identity and stability in practice. Evolving items sit in named modules (`paint`, `pipeline`, `hooks`, `gpu`), so a package's exposure to them can be counted with grep.\n- **Breaks and migration.** Breaks happen only at train boundaries, and each ships a migration note, as Bevy's per-release guides do.\n- **Graduation (Kotlin opt-in precedent).** An item moves into a Stable module of `flui` once it has survived N trains unchanged and has a second consumer. `gpu` and the dev hooks stay Evolving indefinitely.\n- **Verification.** As in Flutter's tests registry, CI builds the packages against the last published train (D7). cargo-semver-checks on the sdk stays advisory, because it misses re-exports and type changes.\n\n**Not verified:**\n- Whether the name `flui-sdk` is free on crates.io; the crates.io MCP server did not connect in this session.\n- Whether cargo-semver-checks can exempt a single module (believed not).\n- The exact resolver error for conflicting `=` pins through a registry; only the E0308 variant with path dependencies was reproduced.\n- The 12-to-20-item size of the Evolving surface, which is a hypothesis to measure with cargo-public-api at W3."
}
```

## judge_q2_sdk_stability_engineer

```json
{
  "choice": "A",
  "confidence": 0.8,
  "reasons": [
    "Build cost: the doc's decision 2 (flui-global-architecture.md:14) measures 191 unique crates in the facade's graph against 127 in flui-material's, and material does not reach wgpu. Options B and C both make every package depend on the host, so they undo that.",
    "Topology churn: material has 14 exact `=0.2.0-dev` pins today (grep -c on crates/flui-material/Cargo.toml). The migration removes flui-tree and flui-localizations and adds flui-reactive and flui-runtime. Under D, each of those changes breaks every package manifest. With an sdk, only the sdk's own manifest changes. Introducing it now costs little, because only first-party packages exist.",
    "Type identity: Probe 1 got an E0308 when the sdk and the facade resolved to different internal versions. That means the sdk can never be versioned independently. It has to move with the train, like wgpu-core, or bump every train, like accesskit_consumer. A `0.N` number tied to one train handles this, and it keeps sdk breaks out of `flui`'s semver.",
    "The doc contradicts itself. Line :204 re-exports the sdk from the Stable facade (`pub use flui_sdk as sdk`), while :209 says the sdk is outside `flui`'s Stable promise. The re-export has to go, or it puts Evolving items into `flui`'s public API, and semver-checks would report sdk churn as breaks of `flui` (the re-export false positives in cargo-semver-checks #638).",
    "Option C does not work with a Cargo feature. Probe 2 showed that feature unification lets an app call the gated module without enabling the feature. A cfg gate instead would push RUSTFLAGS onto every app that uses Material. The same problem applies to `#[cfg(feature = \"unstable\")] pub mod unstable` at :205.",
    "Reversible: folding the sdk into Stable modules later (a move toward B) is additive and can happen item by item. Taking items out of a Stable promise is not reversible after 1.0.",
    "About 12 truly internal items: DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle, FrameSnapshot, InputEpochId, RenderPhysicalShape, RenderTable, TranslationFraction, ElementId::new, RebuildReason and observe. Engine-cpu and flui-runtime have not exercised them yet, so freezing them at H3 (option B) risks a 2.0 soon after 1.0."
  ],
  "conditions": [
    "Delete `pub use flui_sdk as sdk` at flui-global-architecture.md:204. If devtools needs a single path, use `#[doc(hidden)]` or nothing. Replace the `unstable` feature at :205 with `--cfg flui_unstable`, or remove it.",
    "flui-sdk must not depend on flui-app, flui-engine or wgpu. Enforce this with a `forbid-reach` / tier-kind check in `cargo xtask workspace`, not by review. Add a CI assertion that `cargo tree -p flui-sdk -e normal -i wgpu` is empty. The optional `gpu` module goes behind a `dep:` feature, or into a separate crate if it would put wgpu in the default closure.",
    "Stable-closure types in the sdk are pure re-exports, at the same module paths as the facade. The sdk never defines a new type that duplicates a facade type. Evolving items live only in the named modules `paint`, `pipeline`, `hooks` and `gpu`, so a package's exposure to them can be counted with grep.",
    "Every train release, including patch releases, publishes a matching flui-sdk from the same publish run. The sdk pins the internal crates exactly (`=`), and the facade does not depend on the sdk. Before W3, confirm with a local registry (for example cargo-local-registry or an alternate registry) that mismatched trains fail in the resolver, not with E0308.",
    "Before W3, measure the Evolving surface with cargo-public-api. If it goes over about 30 items, beyond the hooks, reopen the decision, because the sdk would then be acting as a second, unstable facade.",
    "CI builds packages/ (Material, Cupertino, devtools) against the last published train, as the ground truth. cargo-semver-checks on flui-sdk stays advisory.",
    "Write down a graduation rule: an item survives N trains unchanged and has a second consumer, then moves into a Stable `flui` module, and the sdk keeps a re-export at the old path. Evolving items change only at train boundaries, each change with a migration note in the train changelog. Record this in an ADR, since it is a cross-crate contract.",
    "Check before committing to the name: flui-sdk availability on crates.io (not verified, because the cratesio MCP server did not connect)."
  ]
}
```

## judge_q2_sdk_stability_ecosystem_author

```json
{
  "choice": "A",
  "confidence": 0.8,
  "reasons": [
    "The contradiction is real. flui-global-architecture.md:204 has `pub use flui_sdk as sdk; // Evolving ...` in the Stable facade listing, while :209 says the sdk is not in the Stable promise of `flui`. A facade re-export makes `flui::sdk::*` reachable by apps and puts it in the facade's public API, so the fix in A (delete :204) is required, not cosmetic.",
    "For a third-party package author at H0-H2, the main risk is manifest churn, not code churn. D (status quo: 14 exact pins across 9 internal crates in flui-material) means every planned topology change (removing flui-tree and flui-localizations, adding flui-reactive and flui-runtime) breaks every external Cargo.toml. That is the bevy_egui pattern, with one release per Bevy release. A single `flui-sdk = \"0.N\"` line absorbs these changes, and the cost of switching is lowest now, while only first-party packages exist.",
    "B fails the package author on build cost and the owner on premature freezing. 127 against 191 crates in the deduplicated cargo tree, and DrawOp, Canvas and RenderUpdateImpact would be frozen before engine-cpu and flui-runtime have exercised them, which leads to a quick 2.0 that breaks app authors too.",
    "C fails as an opt-in. Probe 2 showed Cargo feature unification leaks the gated module to an app that never asked for it, and a `--cfg` gate would force RUSTFLAGS on every Material consumer. It also keeps packages on the full host closure.",
    "App-author DX is unaffected by A. Apps depend on `flui` and `flui-material` only, and re-exporting Stable-closure types under identical paths keeps type identity, so a widget from a package plugs into an app without conversion. The architect's Probe 1 E0308 case is prevented by the exact train pinning A requires.",
    "The sdk's independent number is mostly nominal, since it is locked to the train. That is acceptable: precedent is accesskit_consumer and wgpu-core, its value is keeping Evolving churn out of `flui`'s semver after 1.0, and a train mismatch surfaces at resolve time."
  ],
  "conditions": [
    "Delete `pub use flui_sdk as sdk` at flui-global-architecture.md:204. At most, add a `#[doc(hidden)]` re-export if devtools or templates provably need it, and never a documented path.",
    "Replace or remove `#[cfg(feature = \"unstable\")] pub mod unstable` at :205. Probe 2 showed a Cargo feature does not work as an opt-in, so use `--cfg flui_unstable` or nothing.",
    "Sdk part 1 must be pure re-exports of the same defining items the facade exposes, with identical paths and no wrappers or newtypes, so type identity with `flui` holds. Add a test or check that each `flui_sdk::<stable module>` item is the same item as `flui::<module>`.",
    "Keep the Evolving part narrow and enumerated: named modules paint, pipeline, hooks and gpu only. Measure it with cargo-public-api at W3, with a gate that fails when an item is added without a listed reason. If it grows well past about 20 items plus the hooks, revisit whether those items belong in Stable modules instead.",
    "Sdk breaks happen only at train boundaries. Every train that changes an Evolving module ships a migration note in the train changelog, and every train patch publishes the matching sdk patch in the same automated publish run.",
    "The sdk stays host-free. `cargo xtask workspace` or tier-gate enforcement must confirm flui-sdk's normal dependency closure excludes flui-app, flui-engine and wgpu, with gpu interop behind a clearly named module or feature.",
    "Empirical compatibility gate as in D7. CI builds packages/ (Material, Cupertino, devtools) against the last published train, and cargo-semver-checks on flui-sdk stays advisory.",
    "Write the graduation policy down (an ADR, since this is a cross-crate contract). An item moves into a Stable `flui` module after N unchanged trains plus a second consumer, and the sdk keeps a re-export at the old path. gpu and the dev hooks are declared Evolving indefinitely.",
    "Not verified here: whether the name flui-sdk is free on crates.io (the cratesio MCP server failed to connect this session), and the exact registry resolver error for conflicting `=` pins. Check both before W3."
  ]
}
```

## judge_q2_sdk_stability_owner

```json
{
  "choice": "A",
  "confidence": 0.8,
  "reasons": [
    "The architecture doc contradicts itself. flui-global-architecture.md:204 has `pub use flui_sdk as sdk` inside the Stable facade, while :209 says the sdk 'is not part of the Stable promise of flui'. I checked with sed -n 200,212p. Option A fixes this by deleting :204, and the sdk's churn then stays out of the public API of `flui` and out of semver-checks on it (the #638 re-export class).",
    "B and C make every package build the host: 191 crates for `flui` against 127 for flui-material, measured with deduplicated cargo tree. That breaks decision 2 (:14). Under B, H3 would also freeze about 12 unproven internals (DrawOp, Canvas, RenderUpdateImpact, LocalPostFrameHandle and others), which is the 2.0-soon-after-1.0 trap that the design stance ('fix a bad shape') forbids.",
    "C's Cargo-feature opt-in does not hold. In Probe 2, feature unification let an app call `f2::sdk::draw_op()` without enabling the feature. A cfg-based opt-in would make every consumer of Material set RUSTFLAGS.",
    "D (today: 14 exact pins across 9 internal crates in material, 172 in the workspace) puts internal topology into every package manifest. The migration already plans to delete flui-tree and flui-localizations and to add flui-reactive and flui-runtime, so each topology change would break every package, as Bevy releases do for plugins. Fixing this is cheapest now, while only first-party packages exist.",
    "For a single owner, the extra maintenance is small if the sdk moves in lockstep with the train. Probe 1 (E0308 across differing internal versions) shows it cannot be independent anyway, so it adds one version counter to the same publish run. The sdk still earns its keep by absorbing refactors that would otherwise hit every package.",
    "A is the most reversible choice. It can later be folded into Stable `flui` one item at a time, with flui-sdk left as a deprecated re-export shell. Removing items from a Stable promise, by contrast, costs a major version."
  ],
  "conditions": [
    "Delete `pub use flui_sdk as sdk` (:204) from the facade. Replace `#[cfg(feature = \"unstable\")] pub mod unstable` (:205) with a `--cfg flui_unstable` gate or remove it, because Probe 2 showed the feature gate leaks.",
    "Timing and WIP: do not create flui-sdk before W3, when the tiers land. Until then D stays the interim state, and no docs or README should tell third parties to depend on internal crates.",
    "Keep the Stable-closure half cheap for a single maintainer. Re-export it as whole modules that mirror the facade's module paths (the same `pub use flui_x as x` shape), not as hand-kept item lists, so there is no second catalog to maintain. Only the Evolving modules (`paint`, `pipeline`, `hooks`, `gpu`) list items one by one.",
    "Lockstep versioning: the sdk version is `0.N`, bumped on every train unconditionally and released in the same automated publish run as the train, patches included. There is no independent cadence, which Probe 1 rules out anyway.",
    "Put a ceiling on the Evolving surface. Measure it with cargo-public-api at W3. If it grows well past about 20 items plus the hooks, redesign it (move items into Stable modules or cut them) rather than let the sdk become a second facade.",
    "The package build in CI (D7, Material, Cupertino and devtools built against the published train) is the real compatibility gate. cargo-semver-checks on flui-sdk stays advisory. Any break to an Evolving module ships with a migration note in the train changelog.",
    "Graduation rule after H3: an item moves into a Stable `flui` module after N trains unchanged and once it has a second consumer. `gpu` and the dev hooks may stay Evolving indefinitely.",
    "Still unverified: whether the crate name `flui-sdk` is free on crates.io (the cratesio MCP failed to connect), the exact resolver error for conflicting `=` pins through a registry, and whether cargo-semver-checks can exempt a single module. Check the name before W3."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "Option A claims a train mismatch shows up as a resolver error on `flui-sdk = \"0.N\"` rather than as an E0308. That is false whenever a train bumps the internal crates' 0.x minor, which is the normal breaking train. Under ^0.x rules `internal 0.2` and `internal 0.3` can coexist, so Cargo resolves silently to two copies of the internal crate. The build then fails with exactly the E0308 that A says it avoids.",
      "evidence": "Probe at scratchpad/probe-sdklock uses a directory-source registry with flui 1.0.0 (internal =0.2.0), flui 1.1.0 (internal =0.3.0) and flui-sdk 0.1.0 (internal =0.2.0). The app depends on `flui=\"1\"` and pkg depends on `flui-sdk=\"0.1\"`. `cargo tree` shows `flui v1.1.0 └ internal v0.3.0` next to `flui-sdk v0.1.0 └ internal v0.2.0`. `cargo build` then fails with `error[E0308]: mismatched types / note: there are multiple different versions of crate internal`. The resolver error only appears for patch-level trains: with `flui=\"~1.0.1\"` (=0.2.1) against sdk 0.1.0 (=0.2.0) cargo reports `failed to select a version for internal ... all possible versions conflict`.",
      "severity": "major",
      "fix": "Make two train versions impossible in one graph. Add `links = \"flui_train\"` (plus a trivial build.rs) to one bottom-tier crate every train depends on, such as flui-foundation. Verified in the same probe: with `links` set, the resolver picks one train (`Adding flui v1.0.0 (available: v1.1.0)`, internal v0.2.0 only) and never produces the E0308 graph. Write this into the ADR, and add a registry test to the W3 conditions that shows it."
    },
    {
      "problem": "The 'practical stability' of A's Stable re-export half does not reach package manifests. Every package must move its `flui-sdk = \"0.N\"` requirement on every train, even a package that only uses Stable-closure items, because the sdk bumps 0.N every train and pins the internal crates exactly. So a semver-compatible `flui` 1.x minor release cannot be adopted by any app that uses a package until every such package republishes. The Bevy/bevy_egui churn A cites against D stays in place for all packages, not only for Evolving users. The 'exposure countable by grep' pro measures code exposure, not manifest churn.",
      "evidence": "In the same probe with `links`, an app on `flui=\"1\"` plus a package on `flui-sdk=\"0.1\"` is held at `flui v1.0.0 (available: v1.1.0)`. Without `links`, it breaks with E0308 (previous finding). The architecture doc confirms per-train lockstep: flui-global-architecture.md:209 (`Поломка sdk означает бамп sdk; фасад пинит точную версию`) and §3.4, where exact pins are generated from `[workspace.dependencies]`. The judges accepted lockstep (the owner's condition 'bumped on every train unconditionally'), but none of the pros or costs account for the effect on packages that use only the Stable surface.",
      "severity": "major",
      "fix": "State this cost explicitly in the ADR and in the option's cons: an app's flui minor upgrade waits on package republishes. Mitigations to evaluate at W3: (a) bump the sdk's 0.N only on trains that actually change the Evolving modules or the internal crates' 0.x minor, and publish patch-only trains as 0.N.k; (b) have CI auto-republish first-party packages in the same publish run, which is D7 extended; (c) after H3, consider a host-free Stable crate that stable-only packages can depend on with a caret requirement."
    },
    {
      "problem": "The E0308 evidence from Probe 1, which the rationale uses to argue the sdk 'cannot be independent', applies equally to the facade itself. `flui` 1.x pins the internal crates exactly, and its minor releases move them to a new 0.x. So the Stable facade's own 1.x caret compatibility only holds within a single train. The doc's Stable promise for `flui` needs the same train-coherence mechanism, whatever happens to the sdk.",
      "evidence": "Probe case A: flui 1.1.0 is semver-compatible with `flui=\"1\"`, yet it pulls internal 0.3.0, while anything built against the 1.0 train pulls 0.2.0. The doc says the facade pins exactly (flui-global-architecture.md:209, §3.4 'Внутренние рёбра получают точные пины').",
      "severity": "minor",
      "fix": "Cover it with the same `links` single-train guard, and record in the tier ADR that the internal crates' version moves are what define a train boundary."
    }
  ]
}
```
