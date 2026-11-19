//! Data that leaves one place for another: the clipboard's vocabulary
//! (ADR-0152 §5 and §6, ADR-0337), the data-transfer transport a drop rides
//! ([`data_transfer`], ADR-0312), and the outgoing half of drag-and-drop
//! ([`drag`], ADR-0211).

pub(crate) mod clipboard;
pub mod data_transfer;
pub mod drag;
