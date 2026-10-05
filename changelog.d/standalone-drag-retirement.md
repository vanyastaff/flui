### Fixed
- Preserve independent drag callback ownership across replacement, self-disposal and retirement failures.
- Withdraw terminal pointer tracking before callbacks and diagnostics without erasing reentrant contacts.
- Refuse retained pointer-route delivery into disposed gesture groups.
