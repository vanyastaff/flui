### Removed

- **`flui-widgets`**: the navigator's cross-thread command vocabulary, which nothing sent:
  `NavigatorCommand`, `NavigatorCommandTarget`, `NavigatorCommandOutcome`, `NavigatorCommandError`
  and `NavigatorHandle::command_target`. Navigate on the owner thread through `NavigatorHandle`;
  another thread writes a signal the owner thread reacts to.
