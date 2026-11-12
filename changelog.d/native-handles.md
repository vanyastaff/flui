### Added

- `PlatformProxy::request_exit_policy_reevaluation` asks the owner, from any thread, to ask the exit policy again in its next turn.
- `OwnerPlatform::launch_value`, `data_store`, `data_transfer` and `request_exit_policy_reevaluation`, where their callers already are.

### Removed

- `SharedPlatform` and `OwnerPlatform::shared`: another thread reaches the owner through `PlatformProxy` alone, and the owner-side reads moved to `OwnerPlatform` (ADR-0327 §2).
