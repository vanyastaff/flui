### Added

- `ProvidedValue`, `Provided` and the `provide` hook of `StatelessView` and `ViewState`: a view or
  a state publishes values to its subtree from its own element, and descendants read them with
  `ctx.depend_on::<T, _>(..)` (ADR-0322 §5).

### Changed

- `Focus`, `FocusScope` and `Actions` publish the focus parent and the action chain from their
  own elements; each mounts one element fewer.
- `InheritedElementAccess::view_as_any` is replaced by `provided(type_id)` and `provided_types`,
  and `InheritedBehavior::dependents` is no longer a public field (the `dependents()` method
  stays). `StatelessBehavior` is no longer `Copy`.
