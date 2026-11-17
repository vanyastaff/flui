# ADR-0361: An Apple bundle owns only the file types the system does not declare

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
own format that is right. For `png`, `pdf` or `json` it claims that the application defines and
owns a type macOS and iOS already declare (`public.png`, `com.adobe.pdf`, `public.json`): a second
declaration of the same extension competes with the system's in Launch Services, and `Owner`
asks to be the default handler of every such file on the machine. Apple's guidance is to export
only the types the application defines, and to refer to a type the system declares by its
identifier. Which file types does a bundle export, and how does it open the others?

## Decision drivers

- A bundle claims only what the application defines: it never takes over the system's types.
- One source for both Apple bundles and for `flui doctor`, which reads the plist back.
- The declaration format stays as it is: crates already state extension, MIME type and name.

## Considered options

1. A table of the types Apple's systems declare, keyed by extension; such a type is opened by its
   system identifier as an alternate viewer and not declared; any other is exported and owned.
2. Write every non-own type as an imported declaration (`UTImportedTypeDeclarations`) under its
   system identifier.
3. A new declaration key (`uti = "public.png"`, or `owned = false`) a crate sets for each type it
   does not own.

## Decision

Chosen option: 1, because it needs nothing new from a crate and declares nothing the system does
not need declared.

1. **The system's types are a table.** `APPLE_SYSTEM_TYPES` in `flui-cli`'s
   `declarations/opening.rs` lists the types the Uniform Type Identifiers framework declares as
   `UTType` constants that an application opens as files (images, PDF, plain text, JSON, XML,
   YAML, HTML, CSV, RTF, archives, common audio and video), each with the extensions it claims.
   `FileType::apple_system_type` answers a declared type's system identifier.
2. **A system type is opened, not declared.** Its `CFBundleDocumentTypes` entry names the system
   identifier in `LSItemContentTypes` with `LSHandlerRank` `Alternate`, and no
   `UTExportedTypeDeclarations` or `UTImportedTypeDeclarations` entry is written for it.
3. **Any other type is the application's own.** It is exported as
   `<bundle identifier>.<extension>` with its extension and MIME type, and opened with
   `LSHandlerRank` `Owner`, as before.
4. **The doctor reads the same table.** `flui doctor` counts a document type naming a system
   identifier as opening that identifier's extensions.

### Consequences

- Good, because an application that opens PNG files no longer claims every PNG on the machine,
  and Launch Services keeps one declaration per system type.
- Good, because crates change nothing.
- Bad, because an extension missing from the table is exported and owned under the application's
  identifier even when another application or a newer system declares it; the table grows when
  such a type is declared.
- Neutral, because the MIME type a crate gives a system type is not written to the plist: the
  system's declaration carries it.

### Confirmation

Tests in `flui-cli`: `a_type_the_system_declares_is_opened_and_not_owned` (iOS and macOS keys,
the export list, the rank, and the doctor reading the plist back),
`a_bundle_registers_what_the_application_opens` (an own type is still exported and owned) and
the macOS plist golden `the_macos_info_plist_states_the_application_version`.

## Pros and cons of the options

### 1. A table of system types, opened by identifier

- Good, because nothing is declared that the system already declares, and crates change nothing.
- Bad, because the table is a list to keep.

### 2. Imported declarations for every non-own type

- Good, because the doctor needs no table.
- Bad, because it re-declares system types, which Apple's guidance says not to, and it still
  needs the same table to know which types are the system's.

### 3. A new declaration key

- Good, because a crate can name any type another application owns.
- Bad, because it changes the declaration schema and leaves every existing crate that declares
  `png` claiming it until the crate is edited.

## More information

Apple, "Defining file and data types for your app" (Uniform Type Identifiers documentation): a
type the application defines is exported; a type the system declares is referred to by its
identifier. Tauri writes an exported type only for a file association that sets `exportedType`.
Revisit when crates need to open a type another application owns, which option 3 would serve.
