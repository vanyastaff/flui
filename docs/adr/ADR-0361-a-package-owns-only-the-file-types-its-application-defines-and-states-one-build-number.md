# ADR-0361: A package owns only the file types its application defines, and states one build number

- **Status:** Implemented (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md) §4, the bullet
  "Apple, what the application opens", for its file types only (every declared type exported as
  `<bundle identifier>.<extension>` and opened as its owner); its URL schemes, the iOS
  `LSSupportsOpeningDocumentsInPlace` and the associated-domains entitlement stand
- **Related:** [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md) (where a crate
  declares the file types an application opens)

## Context and problem statement

A crate declares a file type the application opens as an extension, a MIME type and a name
(`[[package.metadata.flui.declares.file-types]]`). Both Apple bundles wrote each one as a uniform
type exported under the bundle identifier (`com.example.notes.png` in
`UTExportedTypeDeclarations`) and opened it with `LSHandlerRank` `Owner`. For the application's
own format that is right. For `png`, `pdf` or `docx` it claims that the application defines and
owns a type macOS and iOS already declare (`public.png`, `com.adobe.pdf`,
`org.openxmlformats.wordprocessingml.document`): a second declaration of the same extension
competes with the system's in Launch Services, and `Owner` asks to be the default handler of every
such file on the machine. The same holds for a format a library crate names on the application's
behalf: the application opens it, it does not define it.

The packages also stated their store build number from the version alone: Android's `versionCode`
as `major * 10000 + minor * 100 + patch`, Apple's `CFBundleVersion` as `major.minor.patch`. Every
pre-release of one version and the release itself then share one build number, and App Store
Connect, Google Play and Sparkle refuse a second upload with a number they already hold. The
limits of the derived `versionCode` were checked only by the Android packager, so a version one
packager refused built a package on another.

Which file types does a bundle claim, and which build number does each package state?

## Decision drivers

- A bundle claims only what the application defines: it never takes over the system's types or
  another party's.
- One source for both Apple bundles and for `flui doctor`, which reads the plist back.
- The declaration format stays as it is: crates already state extension, MIME type and name.
- Every packager accepts the same versions, and every upload a store sees has a build number it
  has not seen.

## Considered options

For file types:

1. A table of the types Apple's systems declare; such a type is opened by its system identifier
   as an alternate viewer; a type the application crate declares is exported and owned; a type
   only a linked crate declares is imported and opened as an alternate viewer.
2. Every type not in the table imported and opened as an alternate viewer, the application's own
   included.
3. A new declaration key (`owned = true`, or `uti = "…"`) a crate sets for each type.

For build numbers:

4. An optional `[app] build` in `flui.toml`, the build number of both stores; without it the
   derived numbers, refused for a release build of a pre-release.
5. A build number derived from the pre-release part (`beta.2` → a fourth component).

## Decision

Chosen options: 1, because it needs nothing new from a crate and declares nothing the system does
not need declared; and 4, because only the project knows which builds a store already holds.

1. **The system's types are a table.** `APPLE_SYSTEM_TYPES` in `flui-cli`'s
   `declarations/opening.rs` lists the types the Uniform Type Identifiers framework declares as
   `UTType` constants that an application opens as files (images, PDF, Office documents, EPUB,
   vCard, iCalendar, plain text, JSON, XML, YAML, HTML, CSS, JavaScript, CSV, RTF, archives,
   common audio and video), each with the extensions it claims. `FileType::apple_system_type`
   answers a declared type's system identifier.
2. **A system type is opened, not declared.** Its `CFBundleDocumentTypes` entry names the system
   identifier in `LSItemContentTypes` with `LSHandlerRank` `Alternate`; no type declaration is
   written for it.
3. **The application crate's own type is exported.** A type the application crate declares and
   the system does not is exported as `<bundle identifier>.<extension>` with its extension and
   MIME type, and opened with `LSHandlerRank` `Owner`. `Declarations` records which types the
   application crate declares (`is_application_type`).
4. **A linked crate's type is imported.** A type only a crate the application links declares,
   unknown to the system, is written as `UTImportedTypeDeclarations` under
   `<bundle identifier>.<extension>` and opened with `LSHandlerRank` `Alternate`; the packager
   warns, naming the extension and the crates.
5. **The doctor reads the same table.** `flui doctor` counts a document type naming a system
   identifier as opening that identifier's extensions.
6. **One build number per package, one rule.** `AppVersion::package_numbers(build, profile)` gives
   every packager its numbers: `CFBundleShortVersionString` is `major.minor.patch`, the version as
   written is `versionName` and `FLUIVersion`, and the build number is `[app] build`
   (`BuildNumber`, 1 to 2 100 000 000) as both `versionCode` and `CFBundleVersion`. Without it
   they are the derived numbers, and a release build of a pre-release is refused with
   `AppVersionError::PreReleaseWithoutBuild`, which names the key to add. `AppVersion::try_new`
   checks the derived `versionCode`'s limits when `flui.toml` loads, so every packager accepts
   the same versions.

### Consequences

- Good, because an application that opens PNG files no longer claims every PNG on the machine,
  and Launch Services keeps one declaration per system type.
- Good, because crates change nothing, and an application still owns the format it defines.
- Good, because a store never receives two builds with one number from a FLUI project.
- Bad, because an extension missing from the table is declared under the application's
  identifier; the table grows when such a type is reported.
- Bad, because a release build of a pre-release needs `[app] build`, which the project raises for
  each upload.
- Neutral, because the MIME type a crate gives a system type is not written to the plist: the
  system's declaration carries it.

### Confirmation

Tests in `flui-cli`: `a_type_the_system_declares_is_opened_and_not_owned` (iOS and macOS keys,
the exported and imported lists, the ranks, the warned types, and the doctor reading the plist
back), `a_bundle_registers_what_the_application_opens`, the macOS plist golden
`the_macos_info_plist_states_the_application_version`, `every_package_states_the_one_version`,
`a_release_of_a_pre_release_needs_a_build_number`,
`a_version_no_packager_takes_is_refused_when_read` and `a_project_file_is_checked_when_it_loads`.

## Pros and cons of the options

### 1. A table of system types, and the application crate's types owned

- Good, because nothing is declared that the system already declares, crates change nothing, and
  the rule matches the one for links: the application declares what is its own.
- Bad, because the table is a list to keep.

### 2. Every type not in the table imported

- Good, because no bundle ever owns a type by mistake.
- Bad, because no application could own the format it defines, and every build would warn about
  it.

### 3. A new declaration key

- Good, because a crate can name any type exactly.
- Bad, because it changes the declaration schema and leaves every existing crate that declares
  `png` claiming it until the crate is edited.

### 4. A declared build number

- Good, because the project, which knows what it has uploaded, decides; a release without a
  pre-release needs nothing.
- Bad, because it is one more value to raise per upload.

### 5. A build number derived from the pre-release

- Good, because nothing is declared.
- Bad, because SemVer pre-release identifiers are free text (`rc`, `beta.2`, `20261117`) with no
  order a store's integer can carry, and a fourth component is not a `versionCode`.

## More information

Apple, "Defining file and data types for your app" (Uniform Type Identifiers documentation): a
type the application defines is exported; a type another party defines is imported; a type the
system declares is referred to by its identifier. Tauri writes an exported type only for a file
association that sets `exportedType`. Revisit when crates need to name the identifier another
application exports for a type, which option 3 would serve.
