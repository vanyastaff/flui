### Changed

- `SnapPolicy` is owner-local like `ScrollPhysics`: it no longer requires `Send + Sync`, and `SharedSnapPolicy` is `Rc<dyn SnapPolicy>` (was `Arc`). Pass `Rc::new(policy)` where `Arc::new(policy)` was passed to `snap_policy`.
