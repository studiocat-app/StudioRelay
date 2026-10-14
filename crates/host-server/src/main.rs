//! Native host for the hosted relay (passthrough mode).
//!
//! Planned: accept TLS connections on :443, read the SNI with `relay-sni`,
//! queue the bytes as a `tcp` stream for the device to pull, serve the device
//! endpoints, verify licenses with `relay-license`, and answer ACME DNS-01
//! challenges for device names. Not implemented yet.

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!(
            "studiorelay-server {} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            relay_protocol::PROTOCOL_VERSION
        );
        return;
    }
    eprintln!("studiorelay-server: not implemented yet");
    std::process::exit(2);
}
