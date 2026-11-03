# ADR-0307: Platform declarations are crate metadata

- **Status:** Accepted (2026-11-03). Partly implemented: the reader, the version-1 keys
  (`android.permissions`, `apple.privacy.accessed-apis`), the generated Android manifest and the
  iOS privacy manifest are in `flui-cli`; the verify mode and `flui add` follow (see Migration).
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0154](ADR-0154-capability-crates.md) §7 only (where declarations live and
  their keys: a versioned `declares` table in any crate, in place of the illustrative schema)
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decision 6 (the generated Android
  manifest); [ADR-0280](ADR-0280-http-is-a-capability-of-the-host.md) (the HTTP capability, whose Android
  stack needs the network permission); [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md)
  (facade features)

## Context and problem statement

An application's manifests have to declare what the code it links needs: a permission on
Android, a usage string, an entitlement or an accessed API on Apple. A missing one fails at run
time, often without saying why. Before this decision FLUI stated its own needs as literals in the
CLI:

| Need | Of | Stated as |
|---|---|---|
| `android.permission.VIBRATE` | `flui-native`'s haptic patterns | a line of the scaffolded `AndroidManifest.xml`, copied once by `flui create` |
| `android.permission.INTERNET` | `flui-app`'s HTTP capability (`HttpURLConnection`) | **nowhere**: every exchange on Android failed with a `SecurityException` |
| `NSPrivacyAccessedAPICategorySystemBootTime`, `…FileTimestamp` | `flui-native`, `flui-assets`, `std::fs` | a constant in `flui-cli`'s iOS packager |

ADR-0154 §7 put declarations in `[package.metadata.flui.capability]`, a table of capability crates
named by `Capability::NAME`. The first crates that need a declaration are not capability crates
but the core host and the built-in capabilities of `flui-app`.

Where does a crate state what it needs from the manifests, so that an application declares
exactly what it links and nothing is a second copy in the CLI?

## Decision drivers

- One source per fact: the crate that needs a declaration states it, next to the code that needs
  it, and the CLI holds no list of anyone's needs.
- A Cargo feature removes its code and its declarations together (ADR-0154 §7).
- Strict: a mistyped key fails the build naming the crate, never a silently missing permission.
- Room to grow without breaking: usage strings, entitlements, background modes, `<queries>`,
  intent filters and MSIX capabilities come with later crates; a crate written for a newer CLI
  must be refused by name, not half-applied.
- No build script patches a manifest (ADR-0154 option 7).

## Considered options

1. Declarations only in capability crates; the core's needs stay literals in the CLI.
2. **A `declares` table any crate of the graph may hold, versioned (chosen).**
3. One registry file in the CLI that maps crate names to their needs.

## Decision

Chosen option: 2.

### 1. Any crate states its needs in `[package.metadata.flui.declares]`

The table may sit in any crate an application links: the core, an official package, a
capability crate, a third party's crate. `[package.metadata.flui.capability] name` stays what
ADR-0154 makes it, the link between a capability crate and `Capability::NAME` for `flui add`; it
holds no declarations.

```toml
[package.metadata.flui.declares]
schema = 1

# Whatever the crate's features.
[package.metadata.flui.declares.android]
permissions = ["android.permission.VIBRATE"]

# Only while the crate's `http` feature is enabled.
[package.metadata.flui.declares.features.http.android]
permissions = ["android.permission.INTERNET"]
```

### 2. Version 1, strict, keys with their first consumer

- `schema` is required. A value the CLI does not read is refused with the crate's name and "update
  flui-cli"; a key the CLI does not know is refused with the crate's name and the key. The reader's
  types use `deny_unknown_fields`. They are private to the `flui` binary, so `#[non_exhaustive]`
  would change nothing: the version number is what keeps the schema open.
- Version 1 has `android.permissions` and `apple.privacy.accessed-apis`, at the top and under
  `features.<feature>`. A permission is two or more dot-separated ASCII Java identifiers, so no
  value can break out of its attribute. An accessed API is a `category`, one of Apple's five
  required-reason categories by its manifest name, and one or more `reasons`, each an
  approved-reason code (four upper-case hexadecimal digits, `.`, a number). Neither is a free
  string.

```toml
[[package.metadata.flui.declares.apple.privacy.accessed-apis]]
category = "NSPrivacyAccessedAPICategoryFileTimestamp"
reasons = ["C617.1"]
```

- A key arrives with the first crate that needs it. A key an older CLI could safely ignore needs no
  new version; one whose absence would build a wrong app raises `schema`.
- `features.<name>` must name a feature of the crate.

### 3. The CLI joins the graph it builds

For each target it builds, the CLI runs `cargo metadata --filter-platform <triple>` in the
application's directory and takes every resolved package's table, plus the parts of the features
the resolved graph enables for it. A dependency only another target links contributes nothing. The
join is a set per key, in a fixed order, with the crates that asked for each entry (`flui build
-v` names them).

### 4. Where it lands

- **Android.** `flui build` writes `<project>/platforms/android/app/src/main/AndroidManifest.xml` whole on
  every build, as ADR-0255 decision 6 asks. It is FLUI's base (`templates/android/AndroidManifest.xml`)
  with the joined permissions, the label from `flui.toml` and the `cdylib` the application builds.
  Both the SDK-tools packager and Gradle read that file. The CLI writes it only when it changed,
  and it carries a generated-file header. A file at that path without the header is refused, not
  overwritten, so a hand edit is never lost silently.
- **Apple.** The iOS packager writes the bundle's `PrivacyInfo.xcprivacy` from the accessed APIs
  the graph of the application's target declares, joined with the application's own
  `<project>/platforms/ios/PrivacyInfo.xcprivacy`: per category, the reasons of every crate and of the
  application. Tracking and collected data stay the application's. A Rust crate links statically
  into the executable, so no per-SDK privacy bundle exists and the application's manifest carries
  the union. That is the rule React Native applies to the pods it links
  (`scripts/cocoapods/privacy_manifest_utils.rb`, `add_aggregated_privacy_manifest`: every pod's
  accessed APIs, the core's and the application's, joined per category with the reasons
  deduplicated), which settles the check ADR-0154 §7 left for this change.

### Consequences

- Good, because an application declares exactly what it links: the `http` feature off, no network
  permission; a crate that reads no file timestamps adds no reason for them.
- Good, because the Android network permission, missing until now, is declared by the crate whose
  provider needs it, and HTTP works on Android.
- Good, because a third party's crate declares its own needs the same way, with no change to the
  CLI.
- Bad, because the manifest is no longer the application's to edit: what it adds (an icon, intent
  filters) has to come through `flui.toml` keys, which arrive with their first need.
- Bad, because each Android build runs `cargo metadata` once per ABI.

### Confirmation

- `declarations::tests::declarations_are_joined_over_the_graph_the_target_links` (a transitive
  declaration, a feature on and one off, a dependency of another target).
- `declarations::tests::a_malformed_declaration_is_refused_by_crate` (an unknown key, a missing or
  newer schema, an unknown feature, a value that is not a permission).
- `android_manifest::tests::the_manifest_declares_what_the_linked_crates_need` reads FLUI's own
  workspace built for Android and finds `VIBRATE` and `INTERNET`, so removing either crate's
  declaration fails it.
- `android_manifest::tests::the_manifest_is_rewritten_only_when_flui_wrote_it`.
- `ios_package::tests::publication_contract` ("privacy manifest declares what flui calls", "privacy
  manifest joins the application's own") reads FLUI's own workspace built for `aarch64-apple-ios`
  and finds `35F9.1` and `C617.1`, joined with an application's reasons.
- `declarations::tests::a_privacy_reason_is_an_approved_reason_code`.
- `cargo xtask workspace` accepts `declares` among the `[package.metadata.flui]` keys.

## Pros and cons of the options

### 1. Declarations only in capability crates

- Good, because it is ADR-0154 §7 as written.
- Bad, because the core's and `flui-app`'s needs stay literals in the CLI, a second copy that an
  application carries whether or not it links the code.
- Bad, because it is how the network permission went missing.

### 2. A `declares` table in any crate, versioned (chosen)

- Good, because the need sits next to the code that has it, and a feature carries both.
- Good, because the version lets a newer crate fail clearly on an older CLI.
- Bad, because every crate's table is read on every Android build.

### 3. A registry in the CLI

- Good, because nothing new appears in crate manifests.
- Bad, because third-party crates cannot be listed, and the registry drifts from the code.

## Migration

1. This change: the reader, `android.permissions`, the generated Android manifest, `flui-native`'s
   `VIBRATE` and `flui-app`'s `http`-feature `INTERNET`.
2. Done: Apple's privacy manifest, `apple.privacy.accessed-apis` in `flui-native` and
   `flui-assets`; the iOS packager's constant is gone, and the application's own
   `PrivacyInfo.xcprivacy` is still joined. The macOS bundle has no privacy manifest yet; when it
   gets one, `flui-native`'s read of the global `NSUserDefaults` (the title bar's double-click
   setting) needs a declared reason.
3. The verify mode, a `flui doctor` check for projects whose manifests the CLI does not write.
4. `flui add <capability>`.

## More information

- Expo's config plugins (typed props with defaults, `false` to remove) remain the model for the
  application's overrides of ADR-0154 §7. They land with the first usage-string key, which is what
  they override.
