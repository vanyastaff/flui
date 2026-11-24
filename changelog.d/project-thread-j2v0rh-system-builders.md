### Changed

- `flui_system::dialog::FileFilter`'s builders take the bare name, as every other description's builder does (ADR-0325 §2): `with_extension`, `with_file_name` and `with_mime_type` are now `extension`, `file_name` and `mime_type`.
