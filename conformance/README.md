# Conformance cases

`cases.json` describes protocol behaviour as steps against one device, so that
every implementation can be checked against the same expectations: the relay
core (`crates/relay-core/tests/conformance.rs`) and the Mac client in the
StudioCat tunnel.

Data is base64. Error, reject and end codes use the protocol's snake_case
names. Steps:

| op | fields | checks |
|---|---|---|
| `hello` | `epoch` | `expectEnded`: what happened to the previous epoch's streams |
| `openHttp` | `deadlineMs` | `expectStream` |
| `clientData` | `stream`, `len`, `data` | — |
| `poll` | `epoch`, `ack` | `expectSeqs` or `expectError` |
| `deviceData` | `epoch`, `stream`, `offset`, `data` | `expectAccepted` (new bytes), `expectReject` or `expectError` |
| `expire` | `nowMs` | `expectExpired` |

Cases are only added, never changed, within a protocol version.
