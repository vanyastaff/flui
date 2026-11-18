### Changed

- **`flui-semantics`**: `SemanticsUpdateCallback` is `Rc<dyn Fn(&TreeUpdate)>` and
  `platform::RouteChangeSink` is `Rc<dyn Fn(RouteChange)>`: the semantics owner and the pipeline
  that holds them never leave their thread, so a callback may capture owner-local state. Pass
  `Rc::new(…)` where `Arc::new(…)` was passed to `SemanticsOwner::new`, `set_callback`,
  `set_route_change_sink` and `PipelineOwner::set_semantics_update_callback`.
