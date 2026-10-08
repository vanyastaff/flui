//! Contracts exercised through the runtime's host interface.

#[path = "contracts/owner_close.rs"]
mod owner_close;
#[path = "contracts/owner_delivery.rs"]
mod owner_delivery;
#[path = "contracts/owner_input.rs"]
mod owner_input;
#[path = "contracts/owner_metrics.rs"]
mod owner_metrics;
#[path = "contracts/owner_publication.rs"]
mod owner_publication;
#[path = "contracts/owner_registry.rs"]
mod owner_registry;
#[path = "../src/table_test.rs"]
mod table_test;
