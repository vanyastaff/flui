### Changed

- `InheritedElement::dependents()` is replaced by `InheritedElement::dependent(id)`, which returns what that element is recorded as reading. A provider keeps its dependents in a list in registration order instead of a hash map, and an inherited scope is a chain of one layer per provider instead of a copy of the nearest providers' types, so a `RawButton` holds 4581 bytes instead of 4710.
