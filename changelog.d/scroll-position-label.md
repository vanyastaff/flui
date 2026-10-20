### Changed

- **`WidgetsLocalizations::scroll_position_label`**: the label of the adjustable position control every
  scroll view publishes comes from the ambient widgets localizations instead of a hard-coded
  English "Scroll position". Implementors of `WidgetsLocalizations` add the string.
