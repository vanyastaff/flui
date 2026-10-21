### Changed

- **`DismissSide`** names the side a `Dismissible` went out toward (`StartToEnd`, `EndToStart`, `Up`,
  `Down`), apart from `DismissDirection`, which stays the set of sides a row allows
  (`DismissDirection::contains`). `Dismissible::on_dismissed` receives a `DismissSide`
  (`DismissDirectionCallback` is now `DismissSideCallback`), and `DismissUpdateDetails::direction` is
  `DismissUpdateDetails::side: Option<DismissSide>`, `None` back at rest.
