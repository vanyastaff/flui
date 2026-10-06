//! flui-platform-api's integration tests, compiled as modules of one binary.

#[path = "composition_ledger.rs"]
mod composition_ledger;

#[path = "lock_gate.rs"]
mod lock_gate;

#[path = "transfer_request.rs"]
mod transfer_request;
