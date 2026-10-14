# StudioRelay device protocol, version 1 (draft)

This document describes how a Mac running StudioCat talks to a StudioRelay
instance. The message types are defined in `crates/relay-protocol` and
published as JSON Schema in `schema/`. Behaviour that every implementation
must share is captured in `conformance/cases.json`.

## Roles

- **Client**: a remote MCP client (Claude, Gemini, Grok, …). It connects to the
  device's public address over HTTPS and never talks to the Mac directly.
- **Relay**: this project. It accepts client connections, queues them per
  device, and enforces rate limits. It does not understand MCP and holds no
  OAuth tokens.
- **Device**: StudioCat's tunnel process on the Mac. It is always the HTTP
  client of the relay: it pulls work and pushes results. The relay never opens
  a connection to the device.

The MCP endpoint and the OAuth authorization server both run on the device.
The relay only carries their traffic.

## Two ways to carry a stream

| Mode | Where TLS ends | What the relay forwards | Used by |
|---|---|---|---|
| `http` | the relay | the HTTP request and response, split into events and frames | self-hosted relays (Cloudflare) |
| `tcp` | the device | raw TLS bytes, routed by the ClientHello's server name | the hosted relay |

The device feeds both into the same HTTP handler.

## Addresses

| Mode | MCP address | OAuth issuer |
|---|---|---|
| self-hosted | `https://<relay host>/d/<deviceId>/mcp` | `https://<relay host>/d/<deviceId>` |
| hosted | `https://<deviceId>.d.relay.studiocat.app/mcp` | `https://<deviceId>.d.relay.studiocat.app` |

The relay returns the address in `hello`; the device uses it as the OAuth
`resource` and does not trust the request's `Host` header.

## Endpoints

All requests are `POST` with JSON bodies. Every endpoint except `register`
requires `Authorization: Device <deviceId>.<deviceSecret>`. Errors use a
non-2xx status with an `ErrorBody` (`schema/error.json`).

### `/v1/device/register`

Exchanges a one-time proof for a device credential.

- Self-hosted: `{"kind": "enroll", "token": …}`, the enrollment token set when
  the relay was deployed. A token registers one device and is then spent.
- Hosted: `{"kind": "license", "token": "SCL1.…"}`, a StudioCat license that
  includes `relay.hosted`. The relay verifies it offline and uses its `sub` as
  the tenant.

The response carries `deviceId` and `deviceSecret`. The relay stores only a
hash of the secret. The device keeps it in the macOS keychain as a
this-device-only item, so it is not synced or migrated to another Mac.
Registering again replaces the credential.

### `/v1/device/hello`

Starts a new epoch and ends the previous one. The response carries `epoch`,
`address` and `limits`. Call it at start-up and whenever a request fails with
`epoch_expired`.

### `/v1/device/poll`

`{"epoch", "ack", "wait"}`. Returns events as soon as there are any, or after
`wait` seconds (at most `limits.maxPollWait`) with an empty list. A device
keeps one or two polls outstanding so that new work is delivered at once.

Events carry consecutive `seq` numbers within an epoch. `ack` acknowledges
every event up to and including that number; events not yet acknowledged are
returned again. A lost poll response therefore loses nothing, and the device
drops events it has already handled by `seq`.

| Event | Meaning |
|---|---|
| `http.open` | a client sent a request: method, path, query, headers, deadline |
| `tcp.open` | a client opened a TLS connection to `sni` |
| `data` | request body or TLS bytes at `offset` (base64) |
| `end` | the client finished sending |
| `reset` | the stream was aborted (`client_gone`, `deadline_exceeded`, …) |
| `throttle` | a rate limit is in effect; informational |

### `/v1/device/send`

`{"epoch", "frames"}`. Frames:

| Frame | Meaning |
|---|---|
| `http.head` | response status and headers (`http` streams) |
| `data` | response body or TLS bytes at `offset` (base64); may be sent in many parts, so streamed responses (SSE) flow as they are produced |
| `end` / `reset` | finish or abort the stream |
| `acme.txt` | hosted mode: publish a DNS-01 challenge for this device's own name |

The relay drops bytes it has already received at the same offset, so a send
that is retried after a network error has no effect. A gap in offsets is
rejected (`offset_gap`).

## Epochs, deadlines and no replay

- A request from an old epoch fails with `epoch_expired`.
- When an epoch ends, nothing is carried into the next one. For `http`
  streams the device had already pulled, the relay answers the client with
  `502` and an MCP error saying the outcome is unknown (the call may have run;
  check its state before retrying). Streams the device never pulled get an
  ordinary error. `tcp` streams are closed.
- `http.open` carries a deadline. A stream still queued at its deadline is
  answered by the relay with `504` and never delivered.

## Limits

Defaults (the relay sends its actual values in `hello`): data chunk 64 KiB,
poll response 1 MiB, 32 concurrent streams per device, request head 32 KiB,
request body 4 MiB, 256 KiB buffered per stream before the relay stops reading
from the client, poll wait 25 s.

## Device offline

- Self-hosted: the relay answers MCP requests with `503` and an MCP error, and
  OAuth endpoints with `503` (the token endpoint returns
  `temporarily_unavailable`, never `invalid_grant`, so clients keep the
  connection).
- Hosted: the relay has no certificate for the device's name and cannot
  complete the TLS handshake; the client sees a connection failure.

## Versioning

`hello.protocol` is the device's version. A relay that cannot serve it returns
`protocol_unsupported` with the versions it supports. Relays support the
current and the previous version for at least six months.
