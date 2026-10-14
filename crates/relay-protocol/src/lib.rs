//! StudioRelay device protocol, version 1.
//!
//! The Mac is always the client. It registers once, then calls `hello` to open
//! an epoch, `poll` to pull events and `send` to push frames. The relay never
//! opens a connection to the Mac. See `docs/protocol-v1.md`.
//!
//! Work is carried in streams. A stream is either an HTTP exchange (the relay
//! ended TLS, self-hosted mode) or raw TCP bytes (TLS ends on the Mac, hosted
//! mode).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// The protocol version this crate speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// Default limits. The relay sends its actual limits in [`HelloResponse`].
pub mod limits {
    /// Largest data chunk in one event or frame, in bytes (before base64).
    pub const MAX_CHUNK: usize = 64 * 1024;
    /// Largest total payload one poll response returns, in bytes.
    pub const MAX_POLL_BYTES: usize = 1024 * 1024;
    /// Concurrent streams per device.
    pub const MAX_STREAMS: u32 = 32;
    /// Largest HTTP request head, in bytes.
    pub const MAX_HEAD: usize = 32 * 1024;
    /// Largest HTTP request body, in bytes.
    pub const MAX_BODY: usize = 4 * 1024 * 1024;
    /// Unread bytes buffered per stream before the relay stops reading the client.
    pub const STREAM_BUFFER: usize = 256 * 1024;
    /// Longest a poll may wait for events, in seconds.
    pub const MAX_POLL_WAIT: u32 = 25;
}

/// Stream identifier, unique within one epoch.
pub type StreamId = u32;

/// Multi-valued header map (lowercase names).
pub type Headers = BTreeMap<String, Vec<String>>;

/// How a stream reaches the Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The relay ended TLS and forwards HTTP exchanges.
    Http,
    /// The relay forwards TLS bytes; the Mac ends TLS.
    Tcp,
}

/// Proof presented once, at registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Proof {
    /// One-time enrollment token set when the relay was deployed (self-hosted).
    Enroll { token: String },
    /// A StudioCat license that includes `relay.hosted` (hosted).
    License { token: String },
}

/// `POST /v1/device/register`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub proof: Proof,
    /// Shown in the relay's device list; never used for routing.
    pub device_name: String,
}

/// The device credential. The relay keeps only a hash of `device_secret`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    pub device_id: String,
    pub device_secret: String,
}

/// `POST /v1/device/hello` — opens a new epoch; the previous one ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct HelloRequest {
    pub protocol: u32,
    pub client_version: String,
    pub modes: Vec<Mode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct HelloResponse {
    pub epoch: u64,
    /// The public MCP address of this device, for example
    /// `https://<id>.d.relay.studiocat.app/mcp`. The Mac uses it as the OAuth
    /// `resource` and never trusts a request's `Host` header instead.
    pub address: String,
    pub limits: Limits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_chunk: u32,
    pub max_poll_bytes: u32,
    pub max_streams: u32,
    pub max_poll_wait: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_chunk: limits::MAX_CHUNK as u32,
            max_poll_bytes: limits::MAX_POLL_BYTES as u32,
            max_streams: limits::MAX_STREAMS,
            max_poll_wait: limits::MAX_POLL_WAIT,
        }
    }
}

/// `POST /v1/device/poll`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PollRequest {
    pub epoch: u64,
    /// Every event with `seq <= ack` has been received and will not be resent.
    pub ack: u64,
    /// Seconds to wait when no event is ready, at most `limits.max_poll_wait`.
    pub wait: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PollResponse {
    pub epoch: u64,
    pub events: Vec<Event>,
}

/// An event pulled by the Mac. `seq` increases by one per event within an
/// epoch; unacknowledged events are sent again on the next poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: u64,
    #[serde(flatten)]
    pub body: EventBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventBody {
    /// A client sent an HTTP request (self-hosted mode).
    #[serde(rename = "http.open", rename_all = "camelCase")]
    HttpOpen {
        stream_id: StreamId,
        method: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        headers: Headers,
        /// Unix milliseconds after which the relay answers the client itself.
        deadline_ms: u64,
    },
    /// A client opened a TLS connection (hosted mode).
    #[serde(rename = "tcp.open", rename_all = "camelCase")]
    TcpOpen { stream_id: StreamId, sni: String },
    /// Request body bytes or TLS bytes, base64.
    #[serde(rename_all = "camelCase")]
    Data {
        stream_id: StreamId,
        offset: u64,
        data: String,
    },
    /// The client finished sending.
    #[serde(rename_all = "camelCase")]
    End { stream_id: StreamId },
    /// The stream was aborted (client gone, deadline passed).
    #[serde(rename_all = "camelCase")]
    Reset {
        stream_id: StreamId,
        code: ResetCode,
    },
    /// A rate limit is in effect; shown to the user, nothing to do.
    #[serde(rename_all = "camelCase")]
    Throttle { limit: String, retry_after_ms: u64 },
}

/// `POST /v1/device/send`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub epoch: u64,
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SendResponse {
    pub epoch: u64,
    /// Indexes into `frames` that were not accepted, with the reason.
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub index: u32,
    pub code: RejectCode,
}

/// A frame sent by the Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Frame {
    /// Response status and headers (HTTP streams only).
    #[serde(rename = "http.head", rename_all = "camelCase")]
    HttpHead {
        stream_id: StreamId,
        status: u16,
        headers: Headers,
    },
    /// Response body bytes or TLS bytes, base64. The relay drops bytes it has
    /// already received at the same offset, so a retried send is harmless.
    #[serde(rename_all = "camelCase")]
    Data {
        stream_id: StreamId,
        offset: u64,
        data: String,
    },
    #[serde(rename_all = "camelCase")]
    End { stream_id: StreamId },
    #[serde(rename_all = "camelCase")]
    Reset {
        stream_id: StreamId,
        code: ResetCode,
    },
    /// Hosted mode: publish an ACME DNS-01 challenge for this device's name.
    #[serde(rename = "acme.txt", rename_all = "camelCase")]
    AcmeTxt { name: String, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResetCode {
    ClientGone,
    DeadlineExceeded,
    TooLarge,
    Refused,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    UnknownStream,
    OffsetGap,
    TooLarge,
    NotAllowed,
}

/// Error body returned by any device endpoint with a non-2xx status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: ErrorCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The device credential is missing or wrong.
    Unauthorized,
    /// The registration proof was rejected.
    BadProof,
    /// The license does not include the hosted relay, or has expired.
    NotEntitled,
    /// The license or device was revoked.
    Revoked,
    /// The request carried an old epoch; call `hello` again.
    EpochExpired,
    ProtocolUnsupported,
    RateLimited,
    BadRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T) -> serde_json::Value
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_value(value).unwrap();
        let back: T = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(&back, value);
        json
    }

    #[test]
    fn events_use_dotted_type_names_and_flat_seq() {
        let event = Event {
            seq: 7,
            body: EventBody::HttpOpen {
                stream_id: 3,
                method: "POST".into(),
                path: "/d/abc/mcp".into(),
                query: None,
                headers: Headers::from([("content-type".into(), vec!["application/json".into()])]),
                deadline_ms: 1_800_000_000_000,
            },
        };
        let json = round_trip(&event);
        assert_eq!(json["type"], "http.open");
        assert_eq!(json["seq"], 7);
        assert_eq!(json["streamId"], 3);
        assert!(json.get("query").is_none());
    }

    #[test]
    fn frames_round_trip() {
        for frame in [
            Frame::HttpHead {
                stream_id: 1,
                status: 200,
                headers: Headers::new(),
            },
            Frame::Data {
                stream_id: 1,
                offset: 0,
                data: "aGk=".into(),
            },
            Frame::End { stream_id: 1 },
            Frame::Reset {
                stream_id: 1,
                code: ResetCode::ClientGone,
            },
            Frame::AcmeTxt {
                name: "_acme-challenge.x".into(),
                value: "v".into(),
            },
        ] {
            round_trip(&frame);
        }
        assert_eq!(round_trip(&Frame::End { stream_id: 2 })["type"], "end");
    }

    #[test]
    fn proof_is_tagged_by_kind() {
        let json = round_trip(&Proof::Enroll { token: "t".into() });
        assert_eq!(json, serde_json::json!({"kind": "enroll", "token": "t"}));
    }

    #[test]
    fn unknown_event_type_is_an_error() {
        let err = serde_json::from_value::<Event>(serde_json::json!({"seq": 1, "type": "nope"}));
        assert!(err.is_err());
    }
}
