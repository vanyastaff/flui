### Changed

- Binding an action to an intent type allocates only the action: its erased form reaches it through a table that exists once per pair of types. `RawButton` binds its three activation intents to one shared action instead of three copies.
- A `Semantics` holds two action handlers inline, and the handler that tells a node where the assistive technology's cursor is exists only for a node that asked to hear it. Mounting a `RawButton` over a `Text` makes 61 allocations, down from 66.
