# ADR-0271: The host font feed asks the platform which faces fall back, and FLUI's tables follow

- **Status:** Accepted (2026-10-30). Not implemented: the platform answers (§3), and the locale-change trigger (§4).
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §7 only for its clause that
  FLUI's tables are the whole fallback order: the platform's answer now comes first, and the
  tables follow it.
- **Related:** [ADR-0270](ADR-0270-the-emoji-generic-starts-with-the-platforms-colour-face.md)
  (the emoji generic, whose table becomes the second source after the platform's answer),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md) (OS types stay in `flui-native`),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (`Since<F>` for the Android API 29 matcher),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no process-global font state)
- **Sources:** `linebender/parley@eea3503dd6cf17130cbb07348e0ff2c918300e94` (fontique 0.11.1)

## Context and problem statement

When a style's family lacks a character, the fallback order decides which face draws it. FLUI's
order is static data: `fallback_tables` holds per-platform lists ported from cosmic-text 0.19, and
`install_into` writes them into the fontique collection under each script's key. The host feed
(`flui-app`, thread `flui-host-fonts`) scans faces with fontdb and installs those lists. Nothing
asks the host.

| Host | What the static lists miss |
|------|----------------------------|
| Linux | The user's and the distribution's fontconfig preferences (`<alias><prefer>`), and any script whose face is not on the recorded list. |
| Windows | Faces that DirectWrite's system fallback picks and the list does not name, such as Features-on-Demand fonts and newer Windows releases. |
| macOS, iOS | The system cascade list for the user's languages (`CTFontCopyDefaultCascadeListForLanguages`), which also changes Han variants by language. |
| Android | `fonts.xml`, the system's own ordered fallback with language tags, plus OEM customisation. The common list is empty today, so Android falls back to its sans-serif face alone. |

A character that no listed face covers shapes as `.notdef` (flui-painting `ARCHITECTURE.md`,
mapping decision 17), so a missing table entry shows as a box (gap 35).

fontique's own `system` feature answers this per `FallbackKey` (script and locale). It queries:
- DirectWrite with each script's sample text (`fontique/src/backend/dwrite.rs:115`);
- CoreText's `CTFontCreateForStringWithLanguage` (`fontique/src/backend/coretext.rs:68`);
- fontconfig's `FcFontMatch` with the script's charset and the locale's langset
  (`fontique/src/backend/fontconfig.rs:735`);
- Android's `fonts.xml` (`fontique/src/backend/android.rs:197`).

FLUI turns that feature off: `parley` and `fontique` are built with `default-features = false`.

How does the fallback order take the host's own answer without putting OS types into
`flui-painting` and without platform calls on the frame path?

## Decision drivers

- A character the host can draw is drawn. A box means the host has no face for it, never that
  FLUI's list lacks the face.
- The frame path is synchronous: no platform call or lock in layout or paint (AGENTS.md).
- OS types (`windows::*`, `objc2::*`, NDK, fontconfig) live in `flui-native` alone (ADR-0082).
  `flui-painting` stays free of a host edge.
- One fallback order for the collection, built off the owner thread by the existing feed. A
  change arrives as a collection generation, the same way a face registration does
  (ADR-0092 §2).
- Identity by typed key, not label. Where the platform names a file, the face is matched by its
  path, not by a family string.
- The order is deterministic in tests: headless, the Web and the testing kit get the same order on
  every host.
- No new process-global state (ADR-0097). Android's floor stays API 23 (ADR-0244).

## Considered options

1. **Keep the static tables** and add entries as boxes are reported.
2. **Turn on fontique's `system` feature** and let fontique discover faces and answer fallback.
3. **A `SystemFallback` value that `flui-native` computes on the feed thread.** `flui-painting`
   merges it ahead of its tables.
4. **Ask the platform per character on a miss**, during layout.

## Decision

Chosen option: 3, because the host's answer is the correct one, while the frame path,
`flui-painting`'s crate edge and test determinism all stay as they are.

### §1 The answer is a value, in `flui-painting`

```rust
/// The host's fallback answer for one locale. Plain data, no handles.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SystemFallback {
    pub locale: String, // BCP 47; FLUI's tables are picked for it too
    pub scripts: Vec<(ScriptCode, Vec<FaceRef>)>,
    pub emoji: Vec<FaceRef>,
    pub common: Vec<FaceRef>,
}

/// An ISO 15924 code: the key a list is kept under.
pub struct ScriptCode([u8; 4]);

/// A face the platform named: by file where it says which, else by family.
pub enum FaceRef {
    File { path: PathBuf, index: u32 },
    Family(String),
}
```

`SystemFallback::none(locale)` means the host gave no answer. `SystemFallback::queries()` lists
every script a list is kept for, with the sample text to ask the platform about; that is fontique's
`SCRIPT_SAMPLES`. `SystemFallback::EMOJI_SAMPLE` is the emoji to ask about.
`HostFonts::scan_with(ask)` calls `ask` once with the host's locale. `HostFonts::scan()` is
`scan_with(SystemFallback::none)`.

### §2 One merge order

`FallbackChain` becomes the merge of the two sources. For each script fontique names, the list is:

1. the host's list for the script;
2. FLUI's table for the script;
3. the host's common list;
4. FLUI's common list;
5. the family that the sans-serif generic names.

The emoji generic is:

1. the host's emoji faces;
2. `fallback_tables::emoji`;
3. the merged common list.

Each face appears once, in the first position where it occurs. Only faces the collection holds
are written. A `FaceRef::File` resolves by file and face index, with both paths canonicalised, so a
path through a symlink still matches. A `FaceRef::Family` resolves through the collection's family
names. A face with colour glyph tables (`COLR`, `CBDT`, `sbix`) never enters a script
list or the common list, only the emoji generic. Without that rule, a platform that ranks an emoji
face high for Latin digits reintroduces the word-gap defect that `font_resolve` prevents.

### §3 `flui-native` computes it, off the owner thread

`flui_native::text::system_fallback(locale: &LanguageTag) -> SystemFallback` exists on every
target. It queries each of fontique's scripts with that script's sample text, plus `😀` for the
emoji faces:

| Target | Per script | Common | Emoji |
|--------|------------|--------|-------|
| Windows | `IDWriteFontFallback::MapCharacters` over the sample, with the locale; one face per script | none | `MapCharacters("😀")` |
| macOS, iOS | `CTFontCreateForStringWithLanguage`, as a URL | `CTFontCopyDefaultCascadeListForLanguages` for the locale | `CTFontCreateForString("😀")` |
| Linux, BSD | `FcFontSort` with the script's charset and the locale's langset; files whose charset covers the sample, in fontconfig's order | none | `FcFontSort` with `color=true` and the emoji charset |
| Android, API 23+ | `fonts.xml` and `fonts_customization.xml` fallback families whose `lang` names the script, as files, in file order | untagged fallback families | the `und-Zsye` family |
| Android, `Since<Api29>` | `AFontMatcher_match` over the sample, ahead of the `fonts.xml` order | as API 23 | as API 23 |
| wasm32, other | `SystemFallback::none(locale)` | | |

fontconfig is opened with `libloading` (`libfontconfig.so.1`). A missing library or symbol gives
`SystemFallback::none(locale)` and one debug log, never a panic. fontconfig never becomes a
link-time dependency.

`flui-app` hands the query to the feed with `HostFontFeed::with_system_fallback(ask)`, and the
feed calls it on `flui-host-fonts` before it scans. The query runs inside the feed's existing
`catch_unwind`. A panic or an OS error in the query logs the first failure and leaves the feed
with no answer, so the static tables still install and the faces still arrive. The query never runs
on the owner thread and never during a frame.

### §4 The answer follows the app's locale

```rust
impl HostFontFeed {
    pub fn run(self) -> FallbackOrder;
}

/// The one writer of a host-fed collection's fallback order. Not `Clone`.
pub struct FallbackOrder { /* the collection */ }

impl FallbackOrder {
    pub fn set_system_fallback(&mut self, system: &SystemFallback) -> bool;
}
```

The feed hands on the collection's one `FallbackOrder` when it has run. `set_system_fallback`
resolves a new answer against the faces the collection holds now and installs it ahead of FLUI's
tables for `system.locale`, replacing the whole order. It raises the collection's generation once,
after every list is written, and does not raise it when nothing changed. A paragraph measured while
the lists were being written therefore measures again at its next frame, and one measured at the
new generation saw the whole new order. If the install unwinds part way, the generation still
rises.

Two installs never interleave their per-script writes. The type enforces this rather than a lock:
there is one feed per collection, and so one `FallbackOrder`, and `set_system_fallback` takes
`&mut self`. The text path takes no FLUI lock (ADR-0092 §3).

When the app's effective locale changes (the locale preference, or a per-app language), the
platform layer asks again for the new locale and installs the answer through its `FallbackOrder`.
Not implemented: the trigger from a locale change waits for the locale preference in the platform
layer ([`docs/gaps.md`](../gaps.md) entry 14). Until then the host locale at start-up holds.

### §5 Tests stay deterministic

Headless hosts, the testing kit and the Web give no answer (`SystemFallback::none`), so a test's
fallback order is FLUI's tables alone, as today. A test that needs a host answer builds a
`SystemFallback` by hand and passes it to `HostFontFeed::with_system_fallback` or
`FallbackOrder::set_system_fallback`.

### Consequences

- Good, because a script the host can draw is drawn on every desktop and mobile host, including
  scripts and faces FLUI's tables never named. Android gets a real fallback order for the first
  time.
- Good, because the user's fontconfig preferences and Apple's per-language cascade (Han variants
  among them) apply.
- Good, because `flui-painting` keeps no host edge and the frame path makes no platform call.
- Bad, because the feed does about 170 platform queries. DirectWrite and CoreText take
  microseconds each; `FcFontSort` takes up to a few milliseconds each. That cost lands on the
  feed thread, which the first frame does not wait for.
- Bad, because four platform implementations need device runs: Windows, macOS, iOS and Android
  are type-checked on CI, and only Linux runs there.
- Neutral, because FLUI's tables stay as the second source and as the whole order where the host
  gives no answer.

### Confirmation

- `the_host_answer_precedes_the_tables` (flui-painting, `fallback_chain`) covers the merge order,
  deduplication, path resolution through `..`, a face the collection lacks, and the colour-face
  exclusion.
- `a_host_fallback_face_draws_a_script_the_tables_miss` (flui-painting, a row of
  `host_feed_contract`) feeds a hand-built `SystemFallback` that names the Arabic probe face, which
  no table names. With the answer the Arabic text shapes in that face; without it, every letter
  is `.notdef`.
- `a_rescan_swaps_the_fallback_order_between_frames` (same table) installs a new answer through
  `FallbackOrder`. Each change raises the generation once, the text measures in the new order, and
  an equal answer changes nothing.
- `fonts_xml_orders_fallback_by_language` (flui-native, `pub(crate)`, runs on Linux) parses
  recorded `fonts.xml` and `fonts_customization.xml` from API 23, 29 and 34.
- `fontconfig_answers_latin_and_emoji` (flui-native, Linux CI) gives a non-empty Latin list and a
  colour emoji face on the CI image. Where fontconfig is absent, it gets the default value.
- A device run on Windows, macOS, iOS and Android renders Arabic, Devanagari, Han in ja-JP and
  zh-CN, and emoji with no missing-glyph box (`docs/platform-parity.md`, gap 35).

## Pros and cons of the options

### 1. Keep the static tables

- Good, because nothing changes and tests are deterministic.
- Bad, because every box is a table edit that waits for someone to report it. User and OEM
  configuration never applies, and Android keeps no common list.

### 2. fontique's `system` feature

- Good, because the answers are already written upstream.
- Bad, because it pulls `windows`, `objc2-core-text` and fontconfig into `flui-painting`, against
  ADR-0082.
- Bad, because fontique's backend answers lazily inside `Query`, which is a platform call on the
  layout path the first time a script appears.
- Bad, because it replaces the fontdb scan and the off-thread feed that ADR-0092 §7 built.
- Bad, because it answers one family per key (`fallback` returns `Option<FamilyId>`), by family
  name. That loses fontconfig's and CoreText's ordered lists and the path identity.

### 3. `SystemFallback` from `flui-native` on the feed thread

- Good, because of the reasons under "Consequences".
- Bad, because FLUI owns four small platform adapters and their fixtures.

### 4. Ask per character on a miss

- Good, because it gives the most precise answer for rare characters.
- Bad, because it is a platform call on the frame path, which the drivers forbid. Parley also
  offers no per-character hook: fallback is per `FallbackKey`.

## More information

- fontique 0.11.1: `fontique/src/backend/dwrite.rs:115`, `fontique/src/backend/coretext.rs:68`
  and `:187`, `fontique/src/backend/fontconfig.rs:735`, `fontique/src/backend/android.rs:197`, and
  `fontique/src/script.rs:37`
  (`SCRIPT_SAMPLES`, the per-script samples this record reuses), at the commit above.
- DirectWrite `IDWriteFontFallback::MapCharacters`:
  <https://learn.microsoft.com/windows/win32/api/dwrite_2/nf-dwrite_2-idwritefontfallback-mapcharacters>.
- CoreText `CTFontCopyDefaultCascadeListForLanguages`:
  <https://developer.apple.com/documentation/coretext/1509992-ctfontcopydefaultcascadelistforla>.
- fontconfig `FcFontSort`: <https://www.freedesktop.org/software/fontconfig/fontconfig-devel/fcfontsort.html>.
- Android NDK `AFontMatcher_match` (API 29):
  <https://developer.android.com/ndk/reference/group/font#afontmatcher_match>.
- Revisit if Parley gains a per-character fallback hook that runs off the frame path.
