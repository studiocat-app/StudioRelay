//! StudioRelay core.
//!
//! Pure state machines with no I/O and no async runtime, so the same code runs
//! inside a Cloudflare Worker (wasm32) and in the native server. Hosts own the
//! sockets, storage and timers; they feed events in and read decisions out.

mod rate;
mod session;

pub use rate::{RateLimited, TokenBucket};
pub use session::{CoreError, DeviceSession, OpenStream, SessionEnd};
