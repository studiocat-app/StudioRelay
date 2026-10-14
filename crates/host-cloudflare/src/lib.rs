//! Cloudflare host for self-hosted relays.
//!
//! Planned: a Worker that ends TLS and routes `/d/<deviceId>/…` to that
//! device's Durable Object, which queues requests as `http` streams and holds
//! the device's pending poll; a registry Durable Object for device
//! credentials. Built for `wasm32-unknown-unknown` with `workers-rs` once the
//! Durable Object support is confirmed. Not implemented yet.

/// Protocol version served by this host.
pub const PROTOCOL_VERSION: u32 = relay_protocol::PROTOCOL_VERSION;
