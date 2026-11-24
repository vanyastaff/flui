# ADR-0397: An icon face is in a binary only where a package or the application asks for it

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) migration step 4a only (the
  clause that `bundled-fonts` installs Material Icons and CupertinoIcons beside Roboto);
  [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md) "Decision"
  only (the clause that the embedded faces, each icon family among them, sit behind
  `bundled-fonts`)
- **Related:** [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (an official package builds
  on `flui-sdk` alone, so a face reaches it through an SDK feature);
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no process-global state, which rules a
  link-time registry out)
- **Sources:** `flutter/flutter@3.50.1` (`ae9b586f34`)
  `packages/flutter_tools/lib/src/flutter_manifest.dart:213` (`usesMaterialDesign`),
  `packages/flutter_tools/lib/src/asset.dart:122-129` (`kMaterialFonts`) and `:627` (the face is
  bundled only under that flag), `packages/flutter/lib/src/cupertino/icons.dart:65-68` (the
  Cupertino face comes from the `cupertino_icons` package),
  `packages/flutter_tools/lib/src/build_system/targets/icon_tree_shaker.dart:30` (`IconTreeShaker`)

## Context and problem statement

`flui-painting` embeds three faces under its default `bundled-fonts` feature, and every
`FontCollection` holds all three. What bears on that:

| Fact | Evidence |
|---|---|
| The faces are Roboto Regular (305,608 bytes), Material Icons (356,840) and Cupertino Icons (257,628). The two icon faces are 614,468 bytes of every application, 2.7% of the generated counter's 22,886,368-byte Linux binary | `crates/flui-painting/assets/fonts/`; release build of `flui create --template counter`, Linux x86_64, 2026-11-23 |
| Roboto is what makes text measure alike on every host: every generic family is bound to it (flui-painting `ARCHITECTURE.md`, mapping decision 16). An icon face takes no part in that rule | `register_embedded_faces` in `crates/flui-painting/src/text_layout/context.rs` |
| No spine crate draws with either icon family. `flui-material` draws three glyphs of Material Icons (`BackButton`, the delete icons of `Chip` and `FilterChip`); `flui-cupertino` is where Cupertino icon data belongs | `packages/flui-material/src/back_button.rs`, `chip.rs` |
| A package builds on `flui-sdk` alone and the process holds no global registry, so a package cannot add a face to collections it never sees from a static | ADR-0088, ADR-0097 |
| `flui::app::register_font` adds a face to a running application: the collection's generation moves and laid-out text measures again on the next frame | `crates/flui-app/src/app/runner/fonts.rs` |
| Flutter bundles its Material face only where the application's manifest sets `uses-material-design: true`, takes the Cupertino face from a package the application depends on, and subsets both to the glyphs a release build names | Sources |

Which faces does a binary hold, and who decides?

## Decision drivers

- An application pays for the faces it draws with, and for no other.
- An author who adds a catalog gets its icons with it: no second step, and nothing in how a widget
  is written changes.
- A face a build holds is in every collection before the first frame: no frame of missing glyphs,
  and no second layout when a face arrives.
- No process-global registry (ADR-0097), and a package names no crate below `flui-sdk` (ADR-0088).
- One place says which faces a build holds, and the compiler reads it.

## Considered options

1. Keep the three faces under `bundled-fonts`.
2. One Cargo feature per icon face, turned on by the package that draws with the family.
3. The package holds the bytes and registers the face when its app widget mounts.
4. The application declares its faces in its manifest and `flui build` bundles them as assets.
5. Subset each face at build time to the glyphs the application names.

## Decision

Chosen option: 2, because it is the only one that removes the bytes from an application that
draws no icon of the family while a face a build does hold stays in every collection from its
construction; Cargo's feature unification is the registry, and it is resolved before the program
runs.

### §1. Each embedded face has its own feature

`flui-painting` declares three features, and `flui_painting::fonts` holds one constant under each:

| Feature | Default | Constant | Family |
|---|---|---|---|
| `bundled-fonts` | on | `ROBOTO_REGULAR` | `"Roboto"`, bound to every generic family |
| `material-icons` | off | `MATERIAL_ICONS_REGULAR` | `"Material Icons"` |
| `cupertino-icons` | off | `CUPERTINO_ICONS` | `"CupertinoIcons"` |

The features are independent: an icon feature without `bundled-fonts` holds that icon face and no
text face. A constant whose feature is off does not exist, so code that names it does not
compile.

### §2. Every collection holds the faces the build embeds

`FontCollection::new`, `with_host_fonts` and `with_host_feed` register each embedded face when the
collection is built. A host feed never adds a host face of a family the collection holds, so an
embedded icon family is never mixed with a host copy; with the feature off the family is the
host's to supply, or the application's through `register_font`.

### §3. The package that draws with a family turns its feature on

`flui-sdk` passes both features through (`material-icons`, `cupertino-icons`). `flui-material`
depends on `flui-sdk` with `material-icons` and `flui-cupertino` with `cupertino-icons`, so the
facade's `material` and `cupertino` features bring the face with the catalog. The facade has the
same two features for an application that draws a family with `flui::widgets::Icon` and no
catalog. A third-party package that draws with one of the families turns the SDK feature on in its
own manifest; a package with its own icon face ships the bytes and documents the
`register_font` call.

### Consequences

- Good, because an application carries only the faces of the catalogs it uses: without a
  catalog none of the 614,468 bytes of the two icon faces (a generated counter measured 592,128
  bytes smaller on Linux x86_64 and 600,277 on wasm32), with Material alone not the Cupertino
  face's 257,628, with Cupertino alone not the Material face's 356,840.
- Good, because an author adds `material` and its icons draw: the feature rides on the dependency.
- Good, because which faces a build holds is decided when Cargo resolves features: no start-up
  order, no registry, no relayout.
- Bad, because features only add: an application that uses a catalog cannot leave its face out.
- Bad, because `Icon` with `"Material Icons"` in an application that has neither the catalog nor
  the facade feature draws the missing-glyph box, where it drew the icon before. `Icon`'s
  documentation and the feature table of the facade say which feature brings the face.
- Neutral, because a workspace build that includes the packages unifies both features on, so a
  test that needs an icon face in a crate below the packages names the feature in its own
  dev-dependency (`flui-engine` does) or registers the bytes on its collection.

### Confirmation

- The compiler: `flui_painting::fonts::MATERIAL_ICONS_REGULAR` and `CUPERTINO_ICONS` are behind
  `#[cfg(feature = …)]`, and the registration names them under the same `cfg`.
- `an_icon_face_reaches_a_consumer_only_with_its_catalog_or_its_feature` (the facade's
  `facade_consumer`): resolves a project outside the workspace that depends on the facade and
  reads `flui-painting`'s features from its graph. With no feature neither icon feature is on;
  `material` and `material-icons` turn on the Material one alone, `cupertino` and
  `cupertino-icons` the Cupertino one alone. A workspace build cannot show this, since its
  members' features are unified.
- `an_icon_of_the_family_shapes_to_a_glyph_of_the_embedded_face`, in `tests/icon_face.rs` of
  `flui-material` and of `flui-cupertino`: in a build that holds the package's face, a codepoint
  of the family shapes to a glyph on a new collection, with nothing registered by hand.
- `a_widget_draws_the_glyph_the_face_names_for_its_icon` (`flui-material`): each icon a widget of
  the package draws by itself is the glyph the embedded face's own ligature names, so a codepoint
  from another numbering of the icon set fails.
- `counter_release_bytes` and `counter_wasm_bytes` in `perf/size.toml`: the generated counter uses
  no catalog. Both faces coming back is 2.7% of it, past the 2% the `size` job allows; one face
  alone is under that, which is why the graph test above holds each.

## Pros and cons of the options

### 1. Keep the three faces under `bundled-fonts`

- Good, because any `Icon` of either family draws in every application.
- Bad, because every application carries 0.61 MB it may never draw with, and the only way out is
  to turn `bundled-fonts` off and lose the text face that makes measurement the same on every
  host.

### 2. One Cargo feature per icon face, turned on by the package that draws with it

- Good, because the cost follows the dependency, and the face is in the collection from its
  construction.
- Good, because the rule is a `cfg`, read by the compiler.
- Bad, because a feature cannot be turned off by a dependent, and an `Icon` whose family no
  feature brings draws the missing-glyph box.

### 3. The package holds the bytes and registers the face when its app widget mounts

- Good, because the bytes live in the package that draws with them.
- Bad, because the face arrives after the collection is built: the first frame can draw missing
  glyphs, and the registration lays every paragraph out again.
- Bad, because a `Chip` used without `MaterialApp` above it has nobody to register for it, and a
  static registry that would is process-global state (ADR-0097).

### 4. The application declares its faces in its manifest and `flui build` bundles them

- Good, because it is the application that decides, as with Flutter's `uses-material-design`.
- Bad, because the face becomes an asset file loaded at start, which `cargo run` and a test
  binary do not have: measurement in a test would differ from the application's.
- Bad, because it makes every author write a line for an icon set the catalog they already chose
  draws with.

### 5. Subset each face at build time to the glyphs the application names

- Good, because it removes the unused glyphs of a face the application does use, which no other
  option does.
- Bad, because it needs the set of codepoints the whole program names, which a build script of
  one crate cannot see; Flutter reads it from the compiled program's constants.
- Neutral, because it composes with option 2 and can follow it: the feature decides whether a
  face is held, a subsetter how much of it.

## More information

Flutter's `Icons` numbers the Material set differently from the upstream
`google/material-design-icons` face FLUI embeds (`arrow_back` is `0xe092` there,
`packages/flutter/lib/src/material/icons.dart:2290`, and `0xE5C4` in the embedded face), so icon
data is written in the embedded face's numbering.

Revisit when a subsetter exists (option 5), or when an application needs a catalog without its
face.
