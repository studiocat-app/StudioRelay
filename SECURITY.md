# Security

## Reporting a vulnerability

Email security@studiocat.app. Please do not open a public issue. We aim to
reply within three working days.

## What the relay stores

| Data | Self-hosted | Hosted | Kept for |
|---|---|---|---|
| Device id, device name | yes | yes | until the device is removed (hosted: 90 days offline) |
| Hash of the device secret | yes | yes | same |
| Tenant (the license's salted email hash) | — | yes | same |
| Connection metadata: time, device id, bytes, result | yes | yes | 30 days |
| Request or response content | passes through, not stored | cannot be read (TLS ends on the Mac) | never stored |
| OAuth tokens | never (tokens are issued and checked on the Mac) | never | — |

## Design notes

- The Mac never listens on a port; it only makes outbound HTTPS requests.
- In hosted mode the relay routes by the TLS server name and forwards bytes it
  cannot decrypt. The operator controls DNS for device names and could in
  principle obtain another certificate for a device; StudioCat can watch
  certificate transparency logs for certificates it did not request.
- Authorizing a client only lets it reach the Mac. What it may do there is
  decided per call by StudioCat's own permissions.
