# StudioRelay

Relay server for [StudioCat](https://studiocat.app).

StudioRelay lets remote MCP clients (Claude, Gemini, Grok and any client that connects to an MCP server by URL and OAuth) reach StudioCat on your Mac without opening a port on the Mac. The Mac pulls work from the relay over outbound HTTPS; the relay never connects to the Mac.

It runs in two ways from the same code:

- **Self-hosted** on your own Cloudflare account.
- **Hosted** by StudioCat at `relay.studiocat.app`, in passthrough mode: TLS ends on your Mac, so the relay cannot read your traffic.

Status: early development. Nothing here is ready to deploy yet.

## License

Apache-2.0. See [LICENSE](LICENSE).
