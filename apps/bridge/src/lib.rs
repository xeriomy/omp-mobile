//! Authenticated loopback bridge for the OMP RPC process.

pub mod api;
pub mod api_types;
pub mod auth;
pub mod config;
pub mod config_store;
pub mod discovery;
pub mod engine;
pub mod process_manager;
pub mod server;

pub use config::BridgeConfig;
pub use server::RunningBridge;
