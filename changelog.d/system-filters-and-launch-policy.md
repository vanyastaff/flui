### Changed

- `ExternalUri::try_new` takes the schemes its caller allows (`&[Scheme::HTTPS, Scheme::HTTP]`): a
  URI whose scheme is not named is refused with `InvalidUri::SchemeNotAllowed`, on every host and in
  the scripted launcher. `smb:`, `nfs:`, `ghelp:`, `ms-msdt:`, `search-ms:` and any other scheme no
  longer pass unless a caller names them; `Scheme::try_new` refuses the schemes no caller may allow.
  A URI is also refused when it holds a character it must percent-encode, such as a space or `"`
  (`InvalidUri::UnencodedCharacter`), a control, bidirectional, zero-width or other hidden character
  (`InvalidUri::HiddenCharacter`, which replaces `ControlCharacter`), writes `https:host` without
  its `//` (`InvalidUri::MissingAuthority`), names a user before its host
  (`https://bank.example@evil.example/`, `InvalidUri::Credentials`), or is a `mailto` whose query
  carries a field other than `to`, `cc`, `bcc`, `subject` and `body` (`InvalidUri::MailtoField`).
  The URI is length-checked before it is copied. `Scheme` is `Hash`.
- `Launcher::open_path` refuses programs, scripts and shortcuts (`.exe`, `.bat`, `.lnk`, `.py`,
  `.desktop`, `.app` and the rest of `RUNNABLE_EXTENSIONS`, and names ending in a Windows class id)
  with `LaunchError::NotADocument`; the Linux launcher also judges the file it opened, refusing a link
  to such a file and an executable. Every path verb refuses a Windows path off the local drives
  (anything but a drive letter) with `LaunchError::NotLocal`. Revealing or trashing such a file still
  works, and the Linux launcher no longer touches the file system on the calling thread.
- A file dialog filter matches extensions in either case on Linux too (`*.png` and `*.PNG`), takes
  extensions of several parts (`tar.gz`), keeps each pattern once in lower case, and takes MIME
  wildcards (`image/*`). An extension holding a wildcard, a separator or a non-ASCII character is
  refused, as are two filters with one name and requests over the new bounds on names, patterns and
  filters (`FileFilter::MAX_NAME_LEN`, `MAX_PATTERNS`, `MAX_PER_REQUEST`).
- `DialogError::InvalidRequest` carries a `MalformedRequest` saying what is wrong.
- A save dialog answers a `SaveLocation`: the file and the index of the filter left selected, where
  the system reports it. The Linux portal is told the first filter starts selected.
- On Linux, the application chooser of a launch is attached to the widget's X11 window.
