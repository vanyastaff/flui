### Fixed

- A stateful view whose parent updates it between its mount and its first build now runs
  `ViewState::init_state` first and then receives one `did_update_view` from the configuration it
  was created with to the current one. Before, `did_update_view` reached a state that had not been
  initialized yet; a `Focus` inside a freshly pushed route hit this and panicked inside frame
  recovery.
