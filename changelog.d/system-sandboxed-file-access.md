### Added

- `OpenRequest::keep_access` hands each pick over with a `flui_system::grant::Grant`
  (`PickedFile::grant`): bytes the application stores and, after a restart, resolves through the
  new `GrantCapability`'s `Grants::resolve` into the file again (`Access`), or a typed
  `GrantError` (`Foreign`, `Revoked`, `Missing`, `TooLarge`). macOS keeps a security-scoped
  bookmark, iOS a bookmark made under the picker's access, Android the content URI with its read
  permission persisted, Linux and Windows the path (a Flatpak's document portal path).
- `flui_system::sandbox::Sandbox` says what the process may open without a grant; `Grants`
  refuses a reacquired path its sandbox does not reach and no started scope holds open.
- `testing::ScriptedGrants` and `testing::GrantStore` simulate a system's grants, a sandbox and a
  restart.

### Changed

- `flui_native::storage::data_dirs` answers a sandboxed process its container: a Flatpak's
  `~/.var/app/<id>/data`, the macOS App Sandbox's container, and a packaged Windows application's
  `LocalState` and `RoamingState` (an MSIX application's stored values move there).
- The Linux user folders answer none for a folder that is not there for the process, as a
  Flatpak shows a folder its manifest does not expose.
