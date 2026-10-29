### Added

- **`flui-widgets`**: `Focus::on_visible_focus_change` reports each edge of visible focus, the
  focus a key, traversal or assistive technology moved, including a modality change with no change
  of primary focus.
- **`flui-material`**: `ThemeData::focus_ring` (and `ThemeDataOverrides::focus_ring`). `Theme`
  provides it as the inherited `DefaultFocusRingStyle`, so every focus ring below a theme,
  `RawButton`'s included, takes its colours; the baselines draw `on_surface` outside `surface`.

### Changed

- **`flui-material`**: `WidgetState::Focused` on an `InkWell` means visible focus: a click that
  focuses a control leaves no overlay, halo or ring. Buttons, tabs, list rows and every other
  `InkWell` draw the theme's focus ring and no longer show a focus state layer; checkboxes,
  switches and radios keep their M3 halo as the indicator
  ([ADR-0079](/docs/adr/ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md)).
