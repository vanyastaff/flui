### Changed

- **`flui-foundation`**: `Marker` and `TreeId` no longer change their thread-safety bounds by target. Concrete ID values retain their existing auto-traits without a cfg-dependent public supertrait.
- **`flui-foundation`**: `FrameEpoch` documentation now names the presentation raster lane that owns and advances it, rather than the scheduler or the removed `Scene::frame_number` field.

### Removed

- **`flui-foundation`**: unused build/platform constants (`DEBUG_MODE`, `RELEASE_MODE`, `IS_WEB`, `IS_MOBILE`, `IS_DESKTOP`), `EPSILON_F32`, `WasmNotSendSync`, `GenerationGate`, and `ResourceGeneration`. Use the owning subsystem's policy and the shared `EPSILON` only where its documented layout tolerance applies.
