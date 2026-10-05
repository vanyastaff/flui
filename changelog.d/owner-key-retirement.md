### Fixed

- Guard independent build-owner and global-key values during exceptional destruction while preserving ordinary retirement order.
- Withdraw local and realm key authority before invoking destructors, clone scope keys outside the registry borrow, and roll back failed admission without calling user key hashing or equality.
