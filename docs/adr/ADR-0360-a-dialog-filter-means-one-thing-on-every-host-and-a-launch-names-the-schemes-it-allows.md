# ADR-0360: A dialog filter means one thing on every host, and a launch names the schemes it allows

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** none. It replaces the launch policy `flui_platform::launch` stated in its own
  documentation (a list of refused schemes, which "an application cannot widen").
- **Superseded-by:** [ADR-0371](ADR-0371-a-dialog-filter-may-name-a-file-exactly-or-admit-every-file.md)
  for §3 only (what a file matches a filter by: also an exact file name, and every file for a
  filter of every file)
- **Related:** [ADR-0154](ADR-0154-capability-crates.md) §4 and §11 (the launcher and the file
  dialogs are `flui-system`'s), [ADR-0344](ADR-0344-a-dialog-lasts-while-its-caller-holds-it-and-ends-with-its-window.md)
  (a dialog's lifetime and the answers it may give), [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md)
  §7 (a launch value from outside is bounded before it is copied),
  [ADR-0339](ADR-0339-an-opened-url-has-one-destination-decided-at-the-runner.md) (the incoming
  side: an opened URL is admitted by the declared schemes)

## Context and problem statement

A file dialog's filter and a launch are where a value from the application, often from its content,
becomes something the operating system acts on. Both had a policy that depended on the host:

- The portal's file chooser matched `*.png` case-sensitively, so a Linux user did not see
  `PHOTO.PNG`, which Windows and macOS show. `with_extension("tar.gz")` was refused, though the
  documentation promised files "ending in `.extension`". An extension could hold `?`, `[` or `;`,
  which each dialog reads differently. A save dialog neither said which filter starts selected nor
  reported the one the user left selected. `DialogError::InvalidRequest` said nothing of what was
  wrong.
- `ExternalUri` refused six schemes and let every other through: `smb:` and `nfs:` (the class of
  Tauri's GHSA-c9pr-q8gx-3mgp, CVE-2025-31477, whose shell `open` let content reach `file:`,
  `smb:` and `nfs:`), `ghelp:` (Yelp's 2025 file read), `ms-msdt:` (Follina), `search-ms:`, and any
  scheme an installed application registers. A link in content could name any of them, and
  `https://bank.example@evil.example/` opens `evil.example`.
- `Launcher::open_path` handed any absolute path to the handler: on Windows a `.exe`, a `.bat` or a
  `.lnk` runs when opened, a Linux `.desktop` entry names a command, a macOS `.app` launches, and
  `\\server\share\doc.pdf` makes Windows connect to a server with the user's credentials.
- The portal launcher sent an empty `parent_window`, so the application chooser was not attached
  to the application's window, while the dialogs attached theirs.

## Decision drivers

- Policy lives at the seam (AGENTS.md "Before each push", item 7): one rule in the contract crate,
  so every backend has it, the scripted double and the headless host included.
- The caller knows what a link may open; the framework cannot. A link to a help page, a
  `mailto:` button and an application's own deep link allow different schemes.
- Types carry the rule: a scheme is a checked value, a refused request names its reason.
- Untrusted input is bounded before it is read or copied.
- A host maps the contract to its OS faithfully; where it cannot, it refuses rather than narrowing
  or widening silently.

## Considered options

1. **Keep the refused list and add to it** (`smb`, `nfs`, `ghelp`, `ms-msdt`, `search-ms`, ...).
2. **A fixed allow-list in the contract** (`http`, `https`, `mailto`, `tel`), with an application
   setting to widen it.
3. **The caller names the schemes it allows on each URI**, over a small list no caller can allow.

For the filters:

A. **Each host keeps its own matching**, documented per host.
B. **One meaning in the contract, an extension grammar that no OS reads as a wildcard, and each
   host maps it.**

## Decision

Chosen options: 3 and B.

Option 1 loses to the next scheme an installed application registers: a refused list is never
complete. Option 2 decides for every call site at once, so a widget that opens only web pages
inherits the application's widening for its deep links. Option 3 makes the allowed set a value at
the call: a link widget that takes text from content names `&[Scheme::HTTPS, Scheme::HTTP]`, and
nothing else reaches the handler. Option A would leave a filter's meaning to the platform an
application happens to run on; B gives a widget one behaviour to test. A file to open has no
caller-named set to allow (any document type is legitimate), so a list of runnable names is the
deny-list option 1 rejects for schemes: it is kept as the floor every host has, and the policy is
each backend asking its own system about the file it opened (§2).

§1. **A launch names the schemes it allows.** `ExternalUri::try_new(uri: &str, allowed: &[Scheme])`.
A `Scheme` is checked (`Scheme::try_new` takes RFC 3986 §3.1 syntax, in ASCII lower case) and
cannot be one of `REFUSED_SCHEMES` (`javascript`, `vbscript`, `data`, `about`, `blob`, `file`);
`Scheme::HTTPS`, `HTTP`, `MAILTO` and `TEL` are named constants; a `Scheme` is `Eq` and `Hash`, so
an allowed set can be a set. A URI passes when, in this order: it is at most
`MAX_EXTERNAL_URI_LEN` bytes, checked before it is read further or copied; it holds no character
RFC 3986 makes it percent-encode (a space, `"`, `<`, `>`, `` ` ``, `{`, `|`, `}`, `^`, `\`:
`UnencodedCharacter`), since Windows splices a URI into its handler's `"<app>" "%1"` command line
as written, which lets a `"` or a space start an argument of the URI's own (the class of
CVE-2018-1000006, worst for an application's own scheme); it holds no character that changes how it
reads without being seen (control, bidirectional formatting, zero-width and invisible formatting
characters, the soft hyphen, the byte-order mark, and any space or separator outside ASCII:
`HiddenCharacter`); it has a valid scheme; the scheme is not refused; the scheme is one of
`allowed`, compared ignoring ASCII case; a scheme with an authority (`http`, `https`, `ftp`, `ws`,
`wss`, `file`) writes the `//` before it (`MissingAuthority`), because a browser reads
`https:bank.example@evil.example` as that authority all the same; the authority names no user
before its host (`Credentials`); and a `mailto` query names only `to`, `cc`, `bcc`, `subject` and
`body`, ignoring ASCII case (`MailtoField`), since some mail clients attach the local file an
`attach` names. A field name is not decoded for the comparison: one holding a `%` is refused. The
URI is handed on as written: nothing decodes its percent-escapes. An application's own declared
schemes (ADR-0307) are what it receives, not what it launches; a caller that launches one names it.

§2. **A path is local, and an open opens documents only.** Every path verb refuses a relative path
(`RelativePath`) and a path off the local drives (`NotLocal`). Locality is an allow-list: on Windows
the only prefixes admitted are a drive letter, plain or verbatim (`Prefix::Disk`,
`Prefix::VerbatimDisk`); every other prefix (`UNC`, `VerbatimUNC`, `DeviceNS`, and `Verbatim`, which
reaches a share through `\\?\GLOBALROOT\Device\Mup\…`) is refused. On every other system an
absolute path has no prefix and is local. Locality is judged twice for an open: at the seam on the
path as given, before any file system call, so a path naming a share never reaches the network; and
by the backend on the final name of the handle it opened, so a link on a local drive that leads to a
share is refused (`NotLocal`) before anything is handed on. A drive the user mapped to a share
resolves to the share (`\\?\UNC\…`), so it is refused for an open; revealing or trashing a file there
is judged on the path as given. Judging the final name comes after the open, which has already
reached a share a link led to: refusing before that connection would mean resolving every component
of the path, links and junctions in any of its folders included, as the system does, which no
backend does.

`open_path` refuses a name that runs when opened (`NotADocument`). The seam decides from the path
alone and touches no file system on the owner thread: the last component ends in one of
`RUNNABLE_EXTENSIONS` or in a Windows class id (`.{…}`), read as Windows reads it (trailing dots and
spaces dropped, a `:stream` suffix ignored). The list (programs, installers, scripts, shortcuts and
searches on Windows, Python and Java scripts, help, theme and sandbox files, archived web pages, a
Linux desktop entry, macOS applications, Terminal scripts, Automator workflows and Finder location
files) is the floor every host has, one list since a file made on one system is opened on another;
it is not the policy. The policy is the backend's own judgement of the file it opened:
`LauncherBackend::open_path` opens the file first, off the owner thread, judges what the descriptor
or handle holds (so a link swapped between a check and the open changes nothing), and answers
`NotADocument` without handing it on when its resolved name `runs_when_opened` or its system calls
it dangerous: on Linux an executable regular file or a desktop entry; on Windows `AssocIsDangerous`
on the final path of the opened handle (`GetFinalPathNameByHandle`); on macOS Launch Services'
application and executable checks. Revealing or trashing such a file runs nothing and is allowed.
Every file system call of a launch (opening, resolving, judging) runs off the owner thread, and its
answer goes to the resolver of the request that asked.

§3. **A filter means one thing.** A file matches a `FileFilter` when its name ends in `.` and one of
its extensions, compared ignoring ASCII case, or when its type is one of its MIME types. An
extension is one or more parts of ASCII letters, digits, `-`, `_` and `+` joined by single dots
(`tar.gz`), so no host reads a character of it as a wildcard, a list separator or a path; a MIME
type is two RFC 6838 names, or a type and `*`, without parameters. Each is at most
`FileFilter::MAX_PATTERN_LEN` bytes, a filter's name at most `FileFilter::MAX_NAME_LEN`, a filter
holds at most `FileFilter::MAX_PATTERNS` patterns and a request at most
`FileFilter::MAX_PER_REQUEST` filters; the counts are checked before anything is copied for the
system. The filter keeps patterns as the dialog compares them (no leading dot, ASCII lower case)
and keeps each once. A request whose filter is blank-named, empty, malformed or over a bound, or
which shows two filters with one name, is refused with
`DialogError::InvalidRequest(MalformedRequest)` naming the reason, before the system is asked.

§4. **A host maps the filter or refuses.** A host whose dialog matches an extension otherwise maps
it (Windows' `*.ext` patterns already ignore case; a dialog that matches only an extension's last
part, as macOS's content types do for `tar.gz`, filters on that part and checks the pick), and one
that cannot express a pattern answers `DialogError::Unsupported` for the request. None drops a
pattern. The portal is the one host that falls short, by a decision: its backends match globs
case-sensitively, and a bracket per letter (`*.[pP][nN][gG]`) would match every case but reach a
save dialog that derives the extension it appends from the selected glob. It sends each extension
in lower and in upper case (`*.png`, `*.PNG`), as GTK applications do, and passes MIME types as they
are; a name in mixed case (`Photo.Png`) on a case-sensitive file system is not shown, which
`docs/gaps.md` records.

§5. **The first filter starts selected, and a save reports the one left selected.** The portal
sends the first filter as `current_filter` as well as first in `filters`. A save answers
`SaveLocation`: the file and, when the system reports it, the index in the request's filters of the
filter selected on accept. The portal reports the selected filter by value, so the backend finds it
by name, which a request holds once; a name the request did not offer, or no report, is `None`. An
index beyond the request's filters is `SystemFailure::MalformedAnswer` at the handle, for every
backend.

§6. **A launch is attached to its window.** The portal launcher takes the acquiring widget's
window, as the dialogs do, and names it as `parent_window` of `OpenURI`, `OpenFile` and
`OpenDirectory` while the window is open; after it closes, or for a widget in no window, the parent
is empty, since a closed window's id may name another window by then.

### Consequences

- Good, because a link from content cannot reach a scheme its widget did not name, on any host, the
  scripted one included.
- Good, because a filter behaves the same on Linux as on Windows and macOS, and a malformed one
  says what is wrong.
- Good, because an application learns which format the user chose when the typed name has no
  extension.
- Bad, because `ExternalUri::try_new`, `DialogError::InvalidRequest` and `SaveResult` change shape:
  a caller names its schemes, matches the `MalformedRequest`, and reads the file from
  `SaveLocation`.
- Bad, because an extension outside ASCII, or one with a wildcard, is refused where the portal
  alone took it before.
- Neutral, because opening a `.js` or `.reg` file in an editor through the launcher is refused on
  every host; an application that means to open one reveals it, or runs a program itself.

### Confirmation

- The compiler: an `ExternalUri` cannot be made without an allowed-scheme list, and a `Scheme`
  cannot be a refused one.
- Tests in `flui-system`'s `system_it`: `external_uri_admits_only_the_schemes_its_caller_names`,
  `a_scheme_is_named_once_whatever_its_case_and_never_a_refused_one`,
  `opening_a_program_a_script_or_a_shortcut_is_refused_without_asking_the_system`,
  `only_a_drive_letter_is_a_local_path` (Windows, a row per `Prefix` kind),
  `a_filter_keeps_each_pattern_once_as_the_dialog_compares_it`,
  `a_malformed_request_is_refused_without_showing_a_dialog`,
  `a_save_reports_the_filter_selected_when_the_user_accepted`; against the stand-in portal on
  Linux, `the_dialogs_reach_the_file_chooser_and_answer_its_picks`,
  `a_save_dialog_reports_the_filter_the_portal_says_was_selected` and
  `a_file_that_would_run_is_judged_by_what_was_opened_and_never_handed_on`; the portal's own
  `a_filter_becomes_globs_then_mime_types` and
  `a_launch_names_the_window_by_its_export_until_the_portal_answers` and
  `a_closed_window_takes_its_exports_with_it_and_names_no_parent`.
