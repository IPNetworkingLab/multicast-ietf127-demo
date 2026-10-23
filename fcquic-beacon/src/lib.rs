use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat<'a> {
    /// Identfier for this heartbeat, e.g., "fcquic-beacon/1".
    #[serde(borrow)]
    pub beacon: Cow<'a, str>,

    /// random number,
    /// each session id identifies a session and allows receiver to detect session restarts from losses
    pub session_id: u32,

    pub seqnum: u64,

    /// time at which this heartbeat was sent as an epoch timestamp
    pub sent_at_ms: u64,

    /// interval between two heartbeats in ms.
    pub interval_ms: u32,
    pub uptime_s: u32,

    /// payload sent after the header, not always present
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Payload>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Payload {
    pub len: u32,

    /// SHA256 of the payload, hex-encoded in JSON.
    #[serde(with = "hex::serde")]
    pub hash: [u8; 32],
}
