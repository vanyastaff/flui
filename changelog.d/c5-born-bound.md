### Removed

- `flui::material::SnackBarActionState` no longer implements `Default`: a state is made only by
  `SnackBarAction::create_state`, which gives it its rebuild handle.
