//! Authenticated loopback bridge for the OMP RPC process.

pub mod api;
pub mod api_types;
pub mod auth;
pub mod config;
pub mod engine;
pub mod server;

pub use config::BridgeConfig;
pub use server::RunningBridge;
