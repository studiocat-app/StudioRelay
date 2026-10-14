//! Runs `conformance/cases.json` against the core.

use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use relay_core::{CoreError, DeviceSession, OpenStream, SessionEnd};
use relay_protocol::{Headers, Limits, RejectCode};
use serde_json::Value;

fn reject_name(code: RejectCode) -> &'static str {
    match code {
        RejectCode::UnknownStream => "unknown_stream",
        RejectCode::OffsetGap => "offset_gap",
        RejectCode::TooLarge => "too_large",
        RejectCode::NotAllowed => "not_allowed",
    }
}

fn error_name(err: CoreError) -> &'static str {
    match err {
        CoreError::EpochExpired => "epoch_expired",
        CoreError::TooManyStreams => "too_many_streams",
    }
}

#[test]
fn conformance_cases() {
    let text = include_str!("../../../conformance/cases.json");
    let doc: Value = serde_json::from_str(text).unwrap();
    for case in doc["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut session: Option<DeviceSession> = None;
        for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let at = format!("{name} / step {i}");
            let u = |k: &str| {
                step[k]
                    .as_u64()
                    .unwrap_or_else(|| panic!("{at}: missing {k}"))
            };
            match step["op"].as_str().unwrap() {
                "hello" => {
                    let next = DeviceSession::new(u("epoch"), Limits::default());
                    let ended = session
                        .replace(next)
                        .map(DeviceSession::end)
                        .unwrap_or_default();
                    if let Some(expect) = step.get("expectEnded") {
                        let got: BTreeMap<String, &str> = ended
                            .iter()
                            .map(|e| match e {
                                SessionEnd::OutcomeUnknown(id) => {
                                    (id.to_string(), "outcome_unknown")
                                }
                                SessionEnd::NotDelivered(id) => (id.to_string(), "not_delivered"),
                                SessionEnd::Close(id) => (id.to_string(), "close"),
                            })
                            .collect();
                        let want: BTreeMap<String, &str> = expect
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap()))
                            .collect();
                        assert_eq!(got, want, "{at}");
                    }
                }
                "openHttp" => {
                    let s = session.as_mut().unwrap();
                    let id = s
                        .open(OpenStream::Http {
                            method: "POST".into(),
                            path: "/mcp".into(),
                            query: None,
                            headers: Headers::new(),
                            deadline_ms: u("deadlineMs"),
                        })
                        .unwrap();
                    assert_eq!(id as u64, u("expectStream"), "{at}");
                }
                "clientData" => {
                    let s = session.as_mut().unwrap();
                    assert!(
                        s.client_data(
                            u("stream") as u32,
                            u("len"),
                            step["data"].as_str().unwrap().into()
                        ),
                        "{at}"
                    );
                }
                "poll" => {
                    let s = session.as_mut().unwrap();
                    match s.poll(u("epoch"), u("ack"), 1 << 20) {
                        Ok(events) => {
                            let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
                            let want: Vec<u64> = step["expectSeqs"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| v.as_u64().unwrap())
                                .collect();
                            assert_eq!(seqs, want, "{at}");
                        }
                        Err(err) => {
                            assert_eq!(Some(error_name(err)), step["expectError"].as_str(), "{at}")
                        }
                    }
                }
                "deviceData" => {
                    let s = session.as_mut().unwrap();
                    let bytes = STANDARD.decode(step["data"].as_str().unwrap()).unwrap();
                    match s.device_data(u("epoch"), u("stream") as u32, u("offset"), &bytes) {
                        Ok(Ok(new)) => {
                            assert_eq!(
                                Some(STANDARD.encode(new).as_str()),
                                step["expectAccepted"].as_str(),
                                "{at}"
                            )
                        }
                        Ok(Err(code)) => assert_eq!(
                            Some(reject_name(code)),
                            step["expectReject"].as_str(),
                            "{at}"
                        ),
                        Err(err) => {
                            assert_eq!(Some(error_name(err)), step["expectError"].as_str(), "{at}")
                        }
                    }
                }
                "expire" => {
                    let s = session.as_mut().unwrap();
                    let got: Vec<u64> = s.expire(u("nowMs")).into_iter().map(u64::from).collect();
                    let want: Vec<u64> = step["expectExpired"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap())
                        .collect();
                    assert_eq!(got, want, "{at}");
                }
                op => panic!("{at}: unknown op {op}"),
            }
        }
    }
}
