### Changed

- `flui_protocol` names each item at one path, through its module: `flui_protocol::Node` is
  `flui_protocol::tree::Node`, `flui_protocol::PROTOCOL_VERSION` is
  `flui_protocol::version::PROTOCOL_VERSION`, and likewise for every module. The crate root
  holds only the method-table macros.
- `flui_protocol` names say what each type is: `session::AppInfo` is `AppDescription`,
  `session::SessionInfo` is `Attached` (what `session.launch` and `session.attach` answer),
  `capture::CaptureMeta` is `Screenshot`, `method::MethodInfo` is `MethodRow`, and the input
  recording's `PointerInfo` is `PointerSource`, as in `flui_platform::input::pointer`. The JSON on the
  wire and the published schema are unchanged.
- `flui_protocol` builders have bare names: `ReadQuery::root`, `max_depth` and `max_nodes`,
  `Tree::diagnostics` and `revision`, `Hello::client`, `HelloReply::app` and `capabilities`,
  `ErrorObject::retry`, `effect` and `handle`, `AuditReport::not_checked` and `Effect::detail`
  replace their `with_` forms.
