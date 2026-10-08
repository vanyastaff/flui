### Added

- Expose `interaction::FocusSubscription` through the package-author SDK for retained focus observers.

### Changed

- Material TextField retains its effective focus-node subscription and withdraws it on node replacement and disposal.
