### Changed

- Value conversion lives in `flui_animation::value_conversion`; value ownership lives in `flui_animation::animated_value`, with motion updates and registry updates in dedicated modules. The former `spring` module is removed; root exports of the value types remain available.
