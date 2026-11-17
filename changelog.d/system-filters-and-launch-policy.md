### Changed

- `ExternalUri::try_new` takes the schemes its caller allows (`&[Scheme::HTTPS, Scheme::HTTP]`): a
  URI whose scheme is not named is refused with `InvalidUri::SchemeNotAllowed`, on every host and in
  the scripted launcher. `smb:`, `nfs:`, `ghelp:`, `ms-msdt:`, `search-ms:` and any other scheme no
  longer pass unless a caller names them; `Scheme::try_new` refuses the schemes no caller may allow.
  A URI that names a user before its host (`https://bank.example@evil.example/`) is refused as
  `InvalidUri::Credentials`, and one holding a bidirectional formatting character as
  `InvalidUri::ControlCharacter`. The URI is length-checked before it is copied.
- `Launcher::open_path` refuses programs, scripts and shortcuts (`.exe`, `.bat`, `.lnk`, `.desktop`,
  `.app` and the rest of `RUNNABLE_EXTENSIONS`), by the path's name and by what it links to, with
  `LaunchError::NotADocument`; every path verb refuses a Windows network share or device path with
  `LaunchError::NotLocal`. Revealing or trashing such a file still works.
- A file dialog filter matches extensions ignoring ASCII case on Linux too (`*.png` shows
  `PHOTO.PNG`), takes extensions of several parts (`tar.gz`), keeps each pattern once in lower case,
  and takes MIME wildcards (`image/*`). An extension holding a wildcard, a separator or a
  non-ASCII character is refused, as are two filters with one name.
- `DialogError::InvalidRequest` carries a `MalformedRequest` saying what is wrong.
- A save dialog answers a `SaveLocation`: the file and the index of the filter left selected, where
  the system reports it. The Linux portal is told the first filter starts selected.
- On Linux, the application chooser of a launch is attached to the widget's X11 window.
