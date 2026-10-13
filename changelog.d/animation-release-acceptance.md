### Fixed

- Keep the Windows input probe alive through manual focus recovery and the remaining input checks. Require its launched process to remain live and own the native window before accepting focus or sending a press, including the shared Notes input helpers.
