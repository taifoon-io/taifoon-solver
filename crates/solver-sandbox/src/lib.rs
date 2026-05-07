//! solver-sandbox library — exposes simulation modules for integration testing.

pub mod compete_sim;
pub mod genome_replay;
pub mod well_sim;
// api is an internal axum handler that depends on axum State types;
// it's not needed by integration tests and is kept binary-only.
