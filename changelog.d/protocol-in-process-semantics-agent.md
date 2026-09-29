### Added

- `flui-protocol` carries the agent-protocol schema of ADR-0080: `ElementId` and `WindowId` handles (`e12`, `w3`), `Node`, `Tree`, `ReadQuery`, `ActionRequest`, the fifteen `ErrorCode`s, `Retry` and the one-line `outline`, versioned apart from the crate as `PROTOCOL_VERSION` (`0.1`) with a published JSON schema per version.
- `flui-semantics` reads its published tree as wire nodes (`SemanticsOwner::read_wire`), with roles, names, state and actions as UI Automation reports them, and resolves a wire action back into a semantics action (`SemanticsOwner::resolve_wire_action`, `semantics_action_for_wire`), refusing an `expand` or `collapse` the node's current state does not allow.
- `flui-runtime` adds `SemanticsAgent`, vended by `UiRealm::semantics_agent`: a `Send + Sync` capability that reads a presentation's committed semantics tree and performs actions on its elements through the realm's owner inbox, answering failures with ADR-0080 codes.
