### Changed
- `SemanticsOwner::resolve_action` refuses a user action on a node published as disabled with `SemanticsActionError::Disabled`, the way the agent surface already did; focus, blur and reveal still reach it. It finds the target through the stable-identity index instead of walking the whole tree.
- `semantics_action_request_for` refuses a request for any tree but `TreeId::ROOT` with `ActionDecodeError::UnknownTree`.
- `GestureBinding::dispatch_contactless` runs its body as a full binding dispatch: a close requested inside waits for it to return, and gesture observers run after it.

### Fixed
- A platform assistive-technology click on a disabled control no longer reaches its handler.
