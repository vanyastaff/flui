# ADR-0371: A dialog filter may name a file exactly or admit every file

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0360](ADR-0360-a-dialog-filter-means-one-thing-on-every-host-and-a-launch-names-the-schemes-it-allows.md)
  §3 only, for what a file matches a filter by (an extension or a MIME type and nothing else); the
  grammar of an extension and of a MIME type, the bounds and the refusals stand
- **Related:** [ADR-0360](ADR-0360-a-dialog-filter-means-one-thing-on-every-host-and-a-launch-names-the-schemes-it-allows.md)
  §4 and §5 (a host maps a filter or refuses, the first filter starts selected),
  [ADR-0344](ADR-0344-a-dialog-lasts-while-its-caller-holds-it-and-ends-with-its-window.md) §4 (an
  answer is one the request can have)
- **Sources:** `PolyMeilex/rfd@2d35d7d07ab4d021ca92e5378ae0e6ed5fbf8d01`
  `src/backend/win_cid/file_dialog/dialog_ffi.rs:219-221` (the `*.*` entry Windows gets when no
  filter is given), `src/backend/xdg_desktop_portal.rs:55-59` (an extension `*` or an empty one
  sent as the glob `*`)

## Context and problem statement

ADR-0360 §3 gave a `FileFilter` one meaning: a file matches when its name ends in `.` and one of
the filter's extensions, or when its type is one of its MIME types, and a filter with neither is
refused. Two things applications do with a file dialog cannot be said in it:

- **"All files" beside specific filters.** A request with no filter shows every file, and a
  request with "Images" shows images only: there is no entry the user selects to reach a file the
  specific filters do not name. `*` is refused as an extension and `*/*` as a MIME type, by design
  (no host reads a pattern's character as a wildcard). rfd's users asked for the entry
  ([rfd#211](https://github.com/PolyMeilex/rfd/issues/211)) and got it as the string `*`, which
  each backend then special-cases.
- **A file with no extension, or with nothing but one.** `Makefile`, `LICENSE` and `Dockerfile`
  have no extension to match by ([rfd#215](https://github.com/PolyMeilex/rfd/issues/215)). `.env`
  and `.gitignore` are a dot and a word: the extension rule asks for a name longer than its
  extension and the dot before it, so no extension names them, while a Windows or portal `*.env`
  pattern shows them, and picking one refused the whole answer with `NotInFilter`.

Scope: `flui-system`'s `FileFilter`, the check `FileDialogs::open` makes of every pick, and each
provider's mapping. What may a filter match by, beside an extension and a MIME type?

## Decision drivers

- A filter means one thing on every host, the scripted one included (ADR-0360 §3).
- No pattern holds a character a host reads as a wildcard, a list separator or a path
  (ADR-0360 §3): a pattern stands for itself.
- Types carry the rule: what a filter is is said by how it is built, not by a string's spelling.
- A host maps a pattern or refuses the request; none drops one (ADR-0360 §4).

## Considered options

1. **Admit `*` as an extension and `*/*` as a MIME type**, each meaning every file.
2. **A typed filter of every file and a typed exact file name**, both matched by the seam's rule.
3. **Glob patterns in the contract** (`Make*`, `*.tar.*`), translated per host.
4. **Leave the filter as it is**: an application that wants every file offers no filter.

## Decision

Chosen option: 2, because both needs are then values of the type, checked like the patterns
ADR-0360 has, and the seam's rule and every host's mapping read them without parsing a spelling.
Option 1 makes one string of the grammar mean something no other does, and every consumer of
`extensions()` has to know it. Option 3 gives up the property that a pattern reads the same on
every host: Windows, the portal's globs and macOS's content types do not agree on a wildcard, which
is why ADR-0360 excluded them. Option 4 leaves both needs unmet.

§1. **A filter of every file.** `FileFilter::all_files(name)` builds a filter that matches every
file; `FileFilter::is_all_files` says so. It holds no pattern, and `with_extension`,
`with_file_name` and `with_mime_type` leave it as it is. It is checked as any filter is for its
name, and is not `EmptyFilter`. A request that offers one admits every pick, whichever filter was
selected on accept: the request offered every file, and no host reports the selection of an open
dialog.

§2. **An exact file name.** `FileFilter::with_file_name(name)` adds a name the filter matches
whole, compared ignoring ASCII case as an extension is. A name is at most
`FileFilter::MAX_PATTERN_LEN` bytes of ASCII letters, digits, `-`, `_`, `+` and `.`, is not `.` or
`..` and does not end in a dot (Windows drops one), so it holds no wildcard, separator or space;
anything else is `MalformedRequest::FileName`. It is kept as written, since a host that compares
names case-sensitively is sent that spelling, and once whatever its case. File names count toward
`FileFilter::MAX_PATTERNS` with extensions and MIME types.

§3. **What a file matches a filter by.** A file matches a `FileFilter` when the filter is one of
every file; or its name ends in `.` and one of the extensions, or is one of the file names, each
compared ignoring ASCII case; or its type is one of the MIME types (`FileFilter::matches`). The
handle holds every pick's name to a request whose filters name extensions and file names only
(`DialogError::NotInFilter` otherwise); a request with a filter of every file, or with a MIME type
among its filters, is not checked by name, as ADR-0360 §4 has it for MIME types.

§4. **Each host maps both.**

| Host | A filter of every file | A file name |
|---|---|---|
| Windows | the pattern `*.*`, which Windows matches against a name with no dot too | the name as its own pattern (`Makefile`), matched whole and ignoring case |
| The portal | the glob `*` | a glob of the name as written, in lower and in upper case; another mix of cases is not shown on a case-sensitive file system, as for an extension |
| macOS | no allowed types: a panel has no filter menu, so every file beside the other filters is every file | no allowed types, since no content type is a name; an open panel's delegate enables what the request's filters name or type, and a save panel takes any name and appends none |

A host whose dialog can express neither (a browser's picker takes MIME types and extensions) shows
every file for such a request, and the handle's check of each pick's name holds the answer to the
filters.

### Consequences

- Good, because an application offers "All files" beside its formats, and a build file or a
  dotfile is a file a dialog can be asked for, with one behaviour to test on every host.
- Good, because a pick named `.env` is admitted by a filter that names it, where an extension
  never could.
- Bad, because a request that offers a filter of every file is no longer held to its other
  filters: the application that offers it takes any file.
- Bad, because on macOS a save panel whose request names a file exactly appends no extension to
  any typed name, since the panel is given no types.
- Neutral, because `FileFilter` gains a third kind of pattern and a flag, and each provider's
  mapping reads both.

### Confirmation

- The compiler: a filter of every file is built by `FileFilter::all_files` only, and a file name
  by `with_file_name` only.
- Tests in `flui-system`'s `system_it`:
  `a_pick_whose_name_ends_in_no_extension_of_the_filters_refuses_the_whole_answer` (rows for a
  filter of every file and for file names), `a_filter_means_a_files_name_or_its_type_ignoring_ascii_case`,
  `a_filter_keeps_each_pattern_once_as_the_dialog_compares_it` and
  `a_malformed_request_is_refused_without_showing_a_dialog` (the `FileName` rows).
- Per host: Windows' `a_filter_becomes_the_dialogs_name_patterns_and_the_first_starts_selected`;
  the portal's `a_file_name_is_its_own_glob_and_every_file_is_one_star` and, against the stand-in
  portal, `a_portal_save_without_an_extension_is_named_after_the_selected_filter`; macOS's
  `a_filter_of_every_file_frees_the_panel_and_a_named_file_leaves_it_to_the_delegate`, a model on
  every host.

## Pros and cons of the options

### 1. `*` as an extension and `*/*` as a MIME type

- Good, because it is what rfd 0.18 and Electron's `filters` spell, so it reads as familiar.
- Bad, because the extension grammar then holds a wildcard after all, and each provider and each
  reader of `extensions()` must special-case one string.
- Bad, because it does nothing for a file with no extension.

### 2. A typed filter of every file and a typed exact file name

- Good, because both are values a provider matches on, and neither can be half-built (a filter of
  every file holds no pattern).
- Good, because a name is a pattern with the same grammar discipline as an extension.
- Bad, because the type grows a constructor, a builder and two accessors.

### 3. Glob patterns in the contract

- Good, because one mechanism covers extensions, names and every file.
- Bad, because the hosts disagree on what a glob means (case, `[…]` sets, `*` across dots), which
  ADR-0360 removed from the contract for that reason, and macOS's content types express none.

### 4. Leave the filter as it is

- Good, because nothing changes.
- Bad, because "All files" beside a format, the most common shape of a filter list, stays
  unsayable, and a dotfile under an extension filter still refuses its own pick.

## More information

- [rfd#211](https://github.com/PolyMeilex/rfd/issues/211),
  [rfd#215](https://github.com/PolyMeilex/rfd/issues/215),
  [rfd#158](https://github.com/PolyMeilex/rfd/issues/158).
- The web provider maps neither kind yet; `docs/gaps.md` says what a browser shows for them.
