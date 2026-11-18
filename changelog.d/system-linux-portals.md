### Added

- `flui::services::system` gains autostart (`Autostart`), global shortcuts (`GlobalShortcuts`, a `ShortcutBinding` that hears the triggers the system chose and each press and release), progress and a count on the application's launcher entry (`TaskbarEntry`) and the user's folders (`UserDirs`). Linux provides them through the `Background` and `GlobalShortcuts` portals, the Unity launcher entry signal and the XDG user directories; Windows provides them too, and the other hosts answer `Unsupported(NotYetOnThisHost)`. `ScriptedSystem` scripts each of them.
- A provider can ask how a desktop portal names the acquiring widget's window (`WindowRef::portal_parent`, `PortalParent`), and which id the application has (`ProviderContext::app_id`).

### Changed

- On Wayland, portal dialogs, application choosers and the new services' questions are attached to the widget's window: the window exports itself (`xdg-foreign`) for each request and the export is released when the request ends, instead of the window being exported for its whole life.
- An unsandboxed Linux application with an id is named to the desktop portal before its first request, so the portal attributes its requests to it.
- A Linux portal too old for a request (an `OpenURI` without `OpenDirectory`, a `FileChooser` without folders) answers `Unsupported(NotOnThisDevice)` before the request is made.
- A portal request or shortcut session still open when its UI runtime ends is closed.
- `WindowRef::new` takes the window (`&Rc<dyn PlatformWindow>`) and its lifetime.
