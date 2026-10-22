### Fixed

- **`flui-native`**: a system notice (sleep, wake, session lock, memory pressure) that a backend
  hands over from another thread just as the previous one finishes delivering reaches the
  `on_host_notice` callback right away. Before, it waited in the queue until the next notice
  arrived.
