### Changed

- **Accessibility handlers attach atomically** (`flui-semantics`, `flui-native`, `flui-runtime`): `PlatformAccessibility::attach` installs one `Arc<dyn AccessibilityHandler>` and returns an `AccessibilityAttachment` with the initial activation state and immutable capabilities. A second attachment fails with `AccessibilityAlreadyAttached` without replacing the live handler; `detach_handler` withdraws it and permits a later presentation to attach. Activation, action requests and assistive-focus moves use required methods of the same handler instead of separate registrations.
