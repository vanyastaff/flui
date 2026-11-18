### Changed

- **`flui-semantics`**: each public type has one path, the crate root. The modules `action`,
  `agent`, `configuration`, `identity`, `node`, `owner`, `properties`, `regions`, `relation`,
  `role`, `states` and `tree` are private; `platform` stays public and is the only path to the
  adapter contract, `AssistiveFocusSupport` included. `TextDirection` and `SemanticsId` are named
  from `flui_foundation`, which owns them. `flui::semantics` still exports
  `AssistiveFocusSupport`.
- **`flui-semantics`**: the agent's tree snapshot keys its id table with `rustc_hash::FxHashMap`,
  and the crate holds that with `clippy::disallowed_types`.
