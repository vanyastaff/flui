### Changed

- `AccessibilityHandler::assistive_focus_changed` is required alongside activation and action delivery, so a handler cannot silently omit assistive-focus delivery. Adapters whose attachment capabilities say `Unreported` do not call it.
