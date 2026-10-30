### Added
- The capability seam (ADR-0152): `flui_platform::capability` declares a capability (`Capability`), its provider (`CapabilityProvider`, given a `ProviderContext`), a `Plugin` that registers providers, and `Completion`/`Resolver` for an answer that arrives later, settled at an owner turn and ended with `Abandoned::OwnerRetired` or `Abandoned::ProviderDropped` when no answer will come. `flui_runtime::capability::CapabilityRegistry` resolves overrides, then built-ins, then plugins, and reports a `CapabilityConflict` when two providers of one tier, or two capabilities of one name, meet. A widget acquires a capability with `cx.capability::<C>()` in a lifecycle hook.
- `Application::capability::<C>(provider)` overrides a built-in provider and `Application::plugin(plugin)` adds a plugin's; a conflict fails the run with `AppRunError::CapabilityConflict` before the platform starts.
- `flui_testing::ScriptedClipboard`, the headless host's clipboard, which can hold, answer or drop reads and refuse to provide; `HeadlessHost::with_capabilities` and `HeadlessHost::with_plugin` run a host over a chosen registry.
- `flui_sdk::platform::capability` and the clipboard items in `flui_sdk::interaction` re-export the seam for packages.

### Changed
- The clipboard is the seam's first capability: `ClipboardCapability` in `flui-interaction`, acquired with `cx.capability::<ClipboardCapability>()`. `ClipboardHandle::read_text` returns a `Completion<Option<String>>` instead of the text; `write_text` stays fire-and-forget.
- `RuntimeHostServices::new` no longer takes an `Arc<dyn Clipboard>`; a UI runtime gets its providers from `RuntimeHostServices::with_capabilities(registry)`, and one without a registry answers every capability with `Unsupported` (`NotRegistered`).
- `Platform::clipboard` returns the owner-local `Rc<dyn NativeClipboard>`, whose read answers through a `Resolver`.
- The Web reads the system clipboard through `navigator.clipboard` asynchronously, falling back to its own writes.

### Removed
- `flui_platform::Clipboard`, `InMemoryClipboard` and `ClipboardItem`, the presentation's `clipboard_handle`, `Platform::write_to_clipboard`/`read_from_clipboard`, and the runtime's `test_clipboard`.
