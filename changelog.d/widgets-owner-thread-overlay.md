### Added

- `OverlayEntry::try_new`, which reports an exhausted identity space as `IdExhausted`; `OverlayEntry::new` keeps panicking there.

### Changed

- `ValueListenableBuilder::new` takes an `Rc<dyn ValueListenable<T>>` and the builder as a closure (`impl Fn(&dyn BuildContext, &T, Option<BoxedView>) -> BoxedView + 'static`) instead of an `Arc` and an `Rc`-wrapped builder.
- The overlay's entry list and entry flags, an `Image`'s published frame and an `ActivityIndicator`'s arc are owner-thread `Rc`/`RefCell`/`Cell` state; route and overlay-entry identities come from the foundation's `AtomicIdMint`.

### Removed

- `ValueWidgetBuilder`: pass the closure to `ValueListenableBuilder::new` directly.
