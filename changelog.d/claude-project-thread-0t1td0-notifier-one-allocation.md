### Changed

- A `Notifier`, and so a `ChangeNotifier` and everything built on one, is one allocation instead of three: its id mint and its disposed flag live in the storage its clones already share. A listener registered through `listen` or `ChangeNotifier::add_listener` is held as it was given, without an adapter allocated around it.
- `FocusNode::with_debug_label` and `FocusScopeNode::with_debug_label` take `impl Into<Cow<'static, str>>`: a literal label is borrowed and allocates nothing. A label built at run time is passed as a `String`.
