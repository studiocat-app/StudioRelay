//! Writes the JSON Schema of every protocol message into `schema/`.
//!
//! `cargo run -p relay-protocol --example export_schema --features schema`

use std::{fs, path::PathBuf};

use relay_protocol::*;
use schemars::{JsonSchema, schema_for};

fn write<T: JsonSchema>(dir: &PathBuf, name: &str) {
    let schema = serde_json::to_string_pretty(&schema_for!(T)).unwrap();
    fs::write(dir.join(format!("{name}.json")), schema + "\n").unwrap();
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    fs::create_dir_all(&dir).unwrap();
    write::<RegisterRequest>(&dir, "register-request");
    write::<RegisterResponse>(&dir, "register-response");
    write::<HelloRequest>(&dir, "hello-request");
    write::<HelloResponse>(&dir, "hello-response");
    write::<PollRequest>(&dir, "poll-request");
    write::<PollResponse>(&dir, "poll-response");
    write::<SendRequest>(&dir, "send-request");
    write::<SendResponse>(&dir, "send-response");
    write::<ErrorBody>(&dir, "error");
}
