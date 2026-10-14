use std::collections::{BTreeMap, VecDeque};

use relay_protocol::{Event, EventBody, Headers, Limits, Mode, RejectCode, ResetCode, StreamId};

/// Errors the core reports to a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// The request carried an epoch other than the current one.
    EpochExpired,
    /// The device already has `limits.max_streams` open streams.
    TooManyStreams,
}

/// A new stream a host wants to hand to the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenStream {
    Http {
        method: String,
        path: String,
        query: Option<String>,
        headers: Headers,
        deadline_ms: u64,
    },
    Tcp {
        sni: String,
    },
}

#[derive(Debug)]
struct StreamState {
    mode: Mode,
    /// Bytes of the client's request the device has been given (next offset).
    sent_to_device: u64,
    /// Bytes of the device's response accepted so far (next offset).
    received_from_device: u64,
    deadline_ms: Option<u64>,
    /// Whether the device has pulled the open event.
    delivered: bool,
    responded: bool,
}

/// What happened to a stream when its epoch ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// The device had the request and may have run it: answer the client with
    /// `outcome_unknown`, never replay it.
    OutcomeUnknown(StreamId),
    /// The device never received the request: safe to answer with an error.
    NotDelivered(StreamId),
    /// A TCP stream: close the client connection.
    Close(StreamId),
}

/// One device's state within one epoch.
///
/// Events get consecutive sequence numbers. The device acknowledges the
/// highest sequence it has received in its next poll; anything not yet
/// acknowledged is returned again, so a lost poll response loses nothing.
#[derive(Debug)]
pub struct DeviceSession {
    epoch: u64,
    limits: Limits,
    next_seq: u64,
    next_stream: StreamId,
    queue: VecDeque<Event>,
    streams: BTreeMap<StreamId, StreamState>,
}

impl DeviceSession {
    pub fn new(epoch: u64, limits: Limits) -> Self {
        Self {
            epoch,
            limits,
            next_seq: 1,
            next_stream: 1,
            queue: VecDeque::new(),
            streams: BTreeMap::new(),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn open_streams(&self) -> usize {
        self.streams.len()
    }

    fn check_epoch(&self, epoch: u64) -> Result<(), CoreError> {
        if epoch == self.epoch {
            Ok(())
        } else {
            Err(CoreError::EpochExpired)
        }
    }

    fn push(&mut self, body: EventBody) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push_back(Event { seq, body });
        seq
    }

    /// Queues a new stream for the device.
    pub fn open(&mut self, open: OpenStream) -> Result<StreamId, CoreError> {
        if self.streams.len() as u32 >= self.limits.max_streams {
            return Err(CoreError::TooManyStreams);
        }
        let stream_id = self.next_stream;
        self.next_stream += 1;
        let (mode, deadline_ms, body) = match open {
            OpenStream::Http {
                method,
                path,
                query,
                headers,
                deadline_ms,
            } => (
                Mode::Http,
                Some(deadline_ms),
                EventBody::HttpOpen {
                    stream_id,
                    method,
                    path,
                    query,
                    headers,
                    deadline_ms,
                },
            ),
            OpenStream::Tcp { sni } => (Mode::Tcp, None, EventBody::TcpOpen { stream_id, sni }),
        };
        self.streams.insert(
            stream_id,
            StreamState {
                mode,
                sent_to_device: 0,
                received_from_device: 0,
                deadline_ms,
                delivered: false,
                responded: false,
            },
        );
        self.push(body);
        Ok(stream_id)
    }

    /// Queues client bytes for the device. `encoded` is the base64 form of
    /// `len` bytes; the core only tracks offsets.
    pub fn client_data(&mut self, stream_id: StreamId, len: u64, encoded: String) -> bool {
        let Some(state) = self.streams.get_mut(&stream_id) else {
            return false;
        };
        let offset = state.sent_to_device;
        state.sent_to_device += len;
        self.push(EventBody::Data {
            stream_id,
            offset,
            data: encoded,
        });
        true
    }

    pub fn client_end(&mut self, stream_id: StreamId) {
        if self.streams.contains_key(&stream_id) {
            self.push(EventBody::End { stream_id });
        }
    }

    /// The client went away: tell the device and forget the stream.
    pub fn client_reset(&mut self, stream_id: StreamId, code: ResetCode) {
        if self.streams.remove(&stream_id).is_some() {
            self.push(EventBody::Reset { stream_id, code });
        }
    }

    /// Handles a poll: drops acknowledged events and returns the rest, up to
    /// `max_bytes` of encoded payload (always at least one event if any).
    pub fn poll(
        &mut self,
        epoch: u64,
        ack: u64,
        max_bytes: usize,
    ) -> Result<Vec<Event>, CoreError> {
        self.check_epoch(epoch)?;
        while self.queue.front().is_some_and(|e| e.seq <= ack) {
            self.queue.pop_front();
        }
        let mut out = Vec::new();
        let mut used = 0usize;
        for event in &self.queue {
            let size = event_size(&event.body);
            if !out.is_empty() && used + size > max_bytes {
                break;
            }
            used += size;
            out.push(event.clone());
        }
        for event in &out {
            if let EventBody::HttpOpen { stream_id, .. } | EventBody::TcpOpen { stream_id, .. } =
                event.body
            {
                if let Some(state) = self.streams.get_mut(&stream_id) {
                    state.delivered = true;
                }
            }
        }
        Ok(out)
    }

    /// Accepts response bytes from the device. Returns the part not seen
    /// before (empty for a retried send), or why the frame was rejected.
    pub fn device_data<'a>(
        &mut self,
        epoch: u64,
        stream_id: StreamId,
        offset: u64,
        bytes: &'a [u8],
    ) -> Result<Result<&'a [u8], RejectCode>, CoreError> {
        self.check_epoch(epoch)?;
        let Some(state) = self.streams.get_mut(&stream_id) else {
            return Ok(Err(RejectCode::UnknownStream));
        };
        if bytes.len() > self.limits.max_chunk as usize {
            return Ok(Err(RejectCode::TooLarge));
        }
        let next = state.received_from_device;
        if offset > next {
            return Ok(Err(RejectCode::OffsetGap));
        }
        let end = offset + bytes.len() as u64;
        if end <= next {
            return Ok(Ok(&[]));
        }
        state.received_from_device = end;
        state.responded = true;
        Ok(Ok(&bytes[(next - offset) as usize..]))
    }

    /// The device finished or aborted a stream.
    pub fn device_close(&mut self, epoch: u64, stream_id: StreamId) -> Result<bool, CoreError> {
        self.check_epoch(epoch)?;
        Ok(self.streams.remove(&stream_id).is_some())
    }

    /// HTTP streams whose deadline passed before the device pulled them. The
    /// host answers the client with 504; the device is told to drop them.
    pub fn expire(&mut self, now_ms: u64) -> Vec<StreamId> {
        let expired: Vec<StreamId> = self
            .streams
            .iter()
            .filter(|(_, s)| !s.delivered && s.deadline_ms.is_some_and(|d| d <= now_ms))
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            self.client_reset(*id, ResetCode::DeadlineExceeded);
        }
        expired
    }

    /// Ends this epoch (a new `hello`, or the device went away). Nothing is
    /// carried into the next epoch: requests are never replayed.
    pub fn end(self) -> Vec<SessionEnd> {
        self.streams
            .into_iter()
            .map(|(id, s)| match s.mode {
                Mode::Tcp => SessionEnd::Close(id),
                Mode::Http if s.delivered || s.responded => SessionEnd::OutcomeUnknown(id),
                Mode::Http => SessionEnd::NotDelivered(id),
            })
            .collect()
    }
}

fn event_size(body: &EventBody) -> usize {
    match body {
        EventBody::Data { data, .. } => data.len(),
        EventBody::HttpOpen { path, headers, .. } => {
            path.len()
                + headers
                    .iter()
                    .map(|(k, v)| k.len() + v.iter().map(String::len).sum::<usize>())
                    .sum::<usize>()
        }
        _ => 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(deadline_ms: u64) -> OpenStream {
        OpenStream::Http {
            method: "POST".into(),
            path: "/d/x/mcp".into(),
            query: None,
            headers: Headers::new(),
            deadline_ms,
        }
    }

    #[test]
    fn unacknowledged_events_are_returned_again() {
        let mut s = DeviceSession::new(1, Limits::default());
        let id = s.open(http(u64::MAX)).unwrap();
        s.client_data(id, 2, "aGk=".into());
        let first = s.poll(1, 0, 1 << 20).unwrap();
        assert_eq!(first.len(), 2);
        // The poll response was lost: the device acks nothing and gets the same events.
        assert_eq!(s.poll(1, 0, 1 << 20).unwrap(), first);
        // Acknowledged events are gone.
        assert!(s.poll(1, first[1].seq, 1 << 20).unwrap().is_empty());
    }

    #[test]
    fn poll_with_old_epoch_is_rejected() {
        let mut s = DeviceSession::new(2, Limits::default());
        assert_eq!(s.poll(1, 0, 1024), Err(CoreError::EpochExpired));
    }

    #[test]
    fn retried_device_data_is_deduplicated() {
        let mut s = DeviceSession::new(1, Limits::default());
        let id = s.open(http(u64::MAX)).unwrap();
        assert_eq!(
            s.device_data(1, id, 0, b"hello").unwrap(),
            Ok(&b"hello"[..])
        );
        assert_eq!(s.device_data(1, id, 0, b"hello").unwrap(), Ok(&b""[..]));
        assert_eq!(
            s.device_data(1, id, 3, b"lo world").unwrap(),
            Ok(&b" world"[..])
        );
        assert_eq!(
            s.device_data(1, id, 20, b"x").unwrap(),
            Err(RejectCode::OffsetGap)
        );
        assert_eq!(
            s.device_data(1, 99, 0, b"x").unwrap(),
            Err(RejectCode::UnknownStream)
        );
    }

    #[test]
    fn stream_limit_is_enforced() {
        let limits = Limits {
            max_streams: 1,
            ..Limits::default()
        };
        let mut s = DeviceSession::new(1, limits);
        s.open(http(u64::MAX)).unwrap();
        assert_eq!(s.open(http(u64::MAX)), Err(CoreError::TooManyStreams));
    }

    #[test]
    fn undelivered_streams_expire_at_their_deadline() {
        let mut s = DeviceSession::new(1, Limits::default());
        // Pulled by the device before the deadline: the device owns the answer.
        s.open(http(1_000)).unwrap();
        s.poll(1, 0, 1 << 20).unwrap();
        // Still queued at the deadline: the relay answers the client itself.
        let undelivered = s.open(http(1_000)).unwrap();
        assert_eq!(s.expire(999), Vec::<StreamId>::new());
        assert_eq!(s.expire(1_000), vec![undelivered]);
        assert_eq!(s.open_streams(), 1);
    }

    #[test]
    fn ending_an_epoch_never_replays() {
        let mut s = DeviceSession::new(1, Limits::default());
        let delivered = s.open(http(u64::MAX)).unwrap();
        s.poll(1, 0, 1 << 20).unwrap();
        let queued = s.open(http(u64::MAX)).unwrap();
        let tcp = s
            .open(OpenStream::Tcp {
                sni: "x.d.relay.studiocat.app".into(),
            })
            .unwrap();
        let mut ends = s.end();
        ends.sort_by_key(|e| match e {
            SessionEnd::OutcomeUnknown(id)
            | SessionEnd::NotDelivered(id)
            | SessionEnd::Close(id) => *id,
        });
        assert_eq!(
            ends,
            vec![
                SessionEnd::OutcomeUnknown(delivered),
                SessionEnd::NotDelivered(queued),
                SessionEnd::Close(tcp),
            ]
        );
    }
}
