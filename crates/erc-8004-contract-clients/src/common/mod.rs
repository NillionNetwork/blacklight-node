//! Common utilities for ERC-8004 contract clients.
//!
//! This module re-exports shared utilities from `contract-clients-common`.

// Re-export everything from the shared crate
pub use contract_clients_common::errors;
pub use contract_clients_common::event_helper;
pub use contract_clients_common::overestimate_gas;
pub use contract_clients_common::tx_submitter;
