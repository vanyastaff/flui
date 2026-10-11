### Added
- Scoped global key registrations withdraw one handler on Drop without retaining the presentation or removing sibling handlers; closed or exhausted owners report explicit registration errors.

### Fixed
- Individual and bulk focus-manager callback withdrawal during a caught observer failure preserve the first failure and avoid hostile capture retirement.
