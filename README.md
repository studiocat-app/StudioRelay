# StudioRelay

Relay server for [StudioCat](https://studiocat.app).

StudioRelay lets remote MCP clients (Claude, Gemini, Grok and any client that connects to an MCP server by URL and OAuth) reach StudioCat on your Mac without opening a port on the Mac. The Mac pulls work from the relay over outbound HTTPS; the relay never connects to the Mac.

It runs in two ways from the same code:

- **Self-hosted** on your own Cloudflare account.
- **Hosted** by StudioCat at `relay.studiocat.app`, in passthrough mode: TLS ends on your Mac, so the relay cannot read your traffic.

Status: early development. Nothing here is ready to deploy yet.

## How it works

1. A remote MCP client connects to your device's address (`https://<relay>/d/<deviceId>/mcp` when self-hosted, `https://<deviceId>.d.relay.studiocat.app/mcp` when hosted).
2. The relay queues the connection for that device.
3. StudioCat on your Mac, which keeps a long poll open to the relay, pulls the request, handles it locally (MCP and OAuth both run on the Mac) and sends the response back.

Authorizing a client happens on your Mac: StudioCat shows who is asking and you approve or deny. The relay never sees or stores OAuth tokens. See [docs/protocol-v1.md](docs/protocol-v1.md).

## Layout

| Path | Contents |
|---|---|
| `crates/relay-protocol` | Message types; exports JSON Schema to `schema/` |
| `crates/relay-core` | Device sessions, streams, delivery, rate limits — no I/O, builds for wasm32 |
| `crates/relay-license` | Offline verification of StudioCat licenses (hosted mode) |
| `crates/relay-sni` | Reads the server name from a TLS ClientHello without ending TLS |
| `crates/host-cloudflare` | Self-hosted relay on Cloudflare Workers (planned) |
| `crates/host-server` | Hosted relay, native passthrough server (planned) |
| `conformance/` | Protocol behaviour cases shared with the Mac client |

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build -p host-cloudflare --target wasm32-unknown-unknown
cargo run -p relay-protocol --example export_schema --features schema
```

## License

Apache-2.0. See [LICENSE](LICENSE).
