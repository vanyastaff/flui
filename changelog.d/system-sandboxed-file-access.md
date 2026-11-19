### Added

- `OpenRequest::keep_access` hands each pick over with a `flui_system::grant::Grant`
  (`PickedFile::grant`): bytes the application stores and, after a restart, resolves through the
  new `GrantCapability`'s `Grants::resolve` into the file again (`Access`), or a typed
  `GrantError` (`Foreign`, `Revoked`, `Missing`, `NotLocal`, `TooLarge`). A resolve lasts while
  its `Resolving` is held; dropping it delivers nothing, stops an Android read and its provider's
  calls, and keeps macOS and iOS from starting the file's security scope (a Linux or Windows
  path check finishes, unheard). macOS keeps a security-scoped
  bookmark, iOS a bookmark made under the picker's access, Android the content URI with its read
  permission persisted, Linux and Windows the path (a Flatpak's document portal path).
- `flui_platform::sandbox` detects the sandbox a process runs in: a Flatpak by its
  `/.flatpak-info` and the folders its manifest exposes, the macOS App Sandbox by its container.
  `flui_system::sandbox::Sandbox` says what the process may open without a grant; `Grants` refuses
  a reacquired path its sandbox does not reach and no started scope holds open.
- `testing::ScriptedGrants` and `testing::GrantStore` simulate a system's grants, a sandbox and a
  restart.

### Changed

- `flui_native::storage::data_dirs` answers a sandboxed process its container: a Flatpak's
  `~/.var/app/<id>/data`, the macOS App Sandbox's container, and a packaged Windows application's
  `LocalState` and `RoamingState`. A host's store finds them at its first call, one find for
  every call that arrives meanwhile, and the host runs the store's calls off the IO workers it
  shares with HTTP, image loads and background work.
  A packaged application that kept a store under `%APPDATA%` before has it copied into the
  package's folders once, at that first call, when the package's folder holds no store yet: files
  are copied, never moved or overwritten, and the copy is put in place whole, by one instance at a
  time (another waits for it). A store that was not copied stays where it is and stays the store:
  the application reads and writes it there. One whose copy failed is copied again at the next
  start; one past the copy's bound on files and bytes is never copied.
- The Linux user folders answer none for a folder that is not there for the process, as a
  Flatpak shows a folder its manifest does not expose.
