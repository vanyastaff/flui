### Changed

- `Localizations::of` and `maybe_of` return `Rc<R>` and no longer ask `R: Send + Sync`; a `LocalizationsDelegate`'s resources and `WidgetsLocalizations` carry no thread bound (ADR-0323 §4).
- `ImageProvider` drops its `Send + Sync` supertraits and `Image` holds it in an `Rc`; `ImageCacheKey::custom` takes a key without a thread bound. `resolve_async` futures stay `Send`: they run on the IO lane.
- `AnimatedList`, `AnimatedGrid` and `SliverAnimatedList` keys no longer need `Send + Sync`.
