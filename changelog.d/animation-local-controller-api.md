### Changed

- Rename `as_listenable()` to `listenable()` on `RefreshController`,
  `ScrollController`, `PageController`, `TransformationController` and
  `ScrollPosition`. Each getter still returns an owned `Rc` sharing the existing
  notification channel. This is an intentional pre-1.0 source break; the older
  animation integration specification's `as_listenable` references use the
  previous spelling of the same channel contract.
- Store local refresh and transformation values in `Cell` and remove refresh
  observation versions that cannot become stale on the owning UI thread.
  Refresh still notifies on every pull write and finish; equal transformation
  writes remain silent. Controller documentation now describes actual `Rc`
  ownership.
