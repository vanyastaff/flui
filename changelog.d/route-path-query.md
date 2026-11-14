### Added

- `RoutePath` carries a query: `RoutePath::query()` reads its pairs in order with repeats,
  `with_query(key, value)` appends one, and `parse` accepts `?k=v&…`, reading `+` as a space.
- `#[derive(Routable)]` reads a field marked `#[query]` (or `#[query(rename = "key")]`) from the
  query: `Option<T>`, `Vec<T>` or any `T: Default`.

### Changed

- `RoutePath::parse` accepts a query; a `#` fragment is still `RouteParseError::Malformed`.
  `RoutePath::prefixes` gives the query to the last prefix only, so `back_stack` opens the pages
  below a deep link without it.
