### Fixed

- Permanently refuse exhausted route and navigator command identities, preserving history and typed command authority after caught capacity failures.
- Guard rejected route and replacement-result ownership and reserve a complete replacement batch before publishing bindings or history.
- Reserve route and overlay identities before a navigation publishes anything, so exhaustion no longer leaves a route's binding filled without an entry, or freezes heroes for a flight that never starts; results and unseeded Router values are retained on refusal.
