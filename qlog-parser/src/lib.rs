use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use qlog::Bytes;

#[derive(Debug)]
pub struct RttSample {
    pub time: SystemTime,
    pub latest_ms: f32,
    pub smoothed_ms: Option<f32>,
}

#[derive(Debug)]
pub struct ReceptionModeUpdate {
    pub time: SystemTime,
    pub old_mode: Option<qlog::events::flexicast::ReceptionMode>,
    pub new_mode: qlog::events::flexicast::ReceptionMode,
    pub trigger: Option<qlog::events::flexicast::ReceptionModeTrigger>,
}

#[derive(Debug)]
pub struct AmtTunnelUpdate {
    pub time: SystemTime,
    pub old: Option<qlog::events::flexicast::AmtTunnelState>,
    pub new: qlog::events::flexicast::AmtTunnelState,
    pub trigger: Option<qlog::events::flexicast::AmtTunnelTrigger>,

    pub relay_ip: Option<String>,
    pub source_ip: Option<String>,
    pub group_ip: Option<String>,
}

#[derive(Debug)]
pub struct ClientStateUpdate {
    pub time: SystemTime,
    pub channel_id: Option<Bytes>,
    /// client id from MC_KEY
    pub client_id: Option<u64>,

    pub old: Option<qlog::events::flexicast::ClientState>,
    pub new: qlog::events::flexicast::ClientState,
}

/// client fallback from the server's point of view
#[derive(Debug)]
pub struct FallBackUpdate {
    pub time: SystemTime,
    pub client_id: Option<u64>,
    pub flow_id: Option<u64>,
    pub action: qlog::events::flexicast::FallbackAction,
    pub trigger: Option<qlog::events::flexicast::FallbackTrigger>,

    /// smoothed RTT, in ms.
    pub receiver_rtt: Option<f32>,
    /// ref RTT that this receiver's rtt was compared against (in ms)
    pub reference_rtt: Option<f32>,
    pub gain_ratio: Option<f32>,
}

/// this event logs a retransmitted stream frame, which could either be duplicated (i.e., we had already received those bytes)
/// or the og frame was lost and the retransmitted frame had a higher offset than we expected
#[derive(Debug)]
pub struct RetransmissionEvent {
    pub time: SystemTime,
    /// packet carrying the retransmission, unicast and fc flow have separate pn spaces (both can start near 0)
    pub packet_number: Option<u64>,
    /// whether that packet came over multicast/AMT, None for qlogs from receivers before the from_mc field
    pub from_mc: Option<bool>,
    pub stream_id: u64,
    pub offset: u64,
    pub length: u64,
    /// true if all the bytes had already been received, i.e., a spurious retransmission
    pub duplicate: bool,
}

/// what the client received so far on a stream
#[derive(Debug, Default)]
pub struct ReceivedStream {
    /// merged received byte ranges {start: end}
    pub ranges: BTreeMap<u64, u64>,
    /// highest packet number that carried data of this stream
    pub max_pn: Option<u64>,
    /// when the first frame of this stream was received
    pub first_frame_time: Option<SystemTime>,
    /// offset + length of the fin frame, i.e., the stream size, None once it's fully delivered to the application
    pub final_size: Option<u64>,
}

#[derive(Debug)]
pub struct StartupPhases {
    /// time between the connection_start event to the mc join event
    pub start_to_mc_join: Option<Duration>,
    /// time between the mc join to the mc timeout, None if the amt fallback timer never expires
    pub join_to_mc_timeout: Option<Duration>,
    /// time between the amt opening event and amt opened events: the time it took for the amt tunnel to be established
    pub amt_opening_to_opened: Option<Duration>,
    /// time between the amt opened event (amt tunnel established) and the first packet received over this tunnel
    pub amt_opened_to_first_amt_pkt: Option<Duration>,
    /// time between the first amt packet received and the first stream frame received
    pub amt_first_packet_to_first_stream_frm: Option<Duration>,
}

#[derive(Debug)]
pub struct ServerUcRetransmission {
    pub time: SystemTime,
    pub stream_id: u64,
    pub length: u64,
}

#[derive(Debug)]
pub struct FcFlowCwnd {
    pub time: SystemTime,
    pub congestion_window: u64,
}

#[derive(Debug)]
pub struct ConnMetrics {
    /// client ip, it will be filled in by the server if the client's own src ip is 0.0.0.0
    pub client_ip: Option<std::net::Ipv4Addr>,
    /// true for the always connected receiver on the dev vm (130.104.229.33), i.e., not a real tester
    pub is_persistent_client: bool,
    /// client's connection_started time
    pub conn_open_time: Option<SystemTime>,
    /// reason from the client's connection_closed event
    pub conn_close_reason: Option<String>,
    /// client's connection_closed time, None if the qlog ended without a close
    pub conn_close_time: Option<SystemTime>,
    /// client rtt samples, on the unicast path only
    pub rtt_samples: Vec<RttSample>,
    /// client reception mode changes (unicast/multicast/amt)
    pub reception_mode_updates: Vec<ReceptionModeUpdate>,
    /// client amt tunnel state changes
    pub amt_tunnel_updates: Vec<AmtTunnelUpdate>,
    /// client state update from the client's pov
    pub client_state_updates: Vec<ClientStateUpdate>,
    /// client state updates from the server's point of view
    pub server_pov_client_state_updates: Vec<ClientStateUpdate>,
    /// server-side fallbacks of this client to unicast (not the amt fallback)
    pub fallback_updates: Vec<FallBackUpdate>,
    /// retransmissions received by the client, from the client qlog
    pub retransmission_events: Vec<RetransmissionEvent>,
    /// gaps in the from_mc packet numbers seen by the client, 0 for old qlogs without from_mc
    pub multicast_losses: u64,
    /// packets received by the client with from_mc, 0 for old qlogs without from_mc
    pub total_multicast_packets: u64,
    /// multicast_losses / (received + lost), None if no from_mc packet was seen
    pub mcast_loss_rate: Option<f64>,
    /// multicast losses from the server's pov (gaps in the fc flow packets acked by the client), also works for old qlogs without from_mc
    pub server_pov_multicast_losses: Option<u64>,
    /// server_pov_multicast_losses / span of acked fc flow packet numbers
    pub server_pov_mcast_loss_rate: Option<f64>,
    /// raw bytes of all packets received by the client, unicast and multicast
    pub client_received_bytes: u64,
    /// raw bytes of all packets sent by the client
    pub client_sent_bytes: u64,
    /// connection_started to the first stream frame received, over unicast or multicast
    pub time_to_first_data: Option<Duration>,
    /// breakdown of the startup up to the first stream frame
    pub startup_phases: StartupPhases,
    /// gaps between consecutive multicast packets longer than 1.5 fc_ping intervals
    pub large_mc_packet_gaps: Vec<Duration>,
    /// first frame to fully read by the app, for streams over 1350 bytes (heartbeats with the payload)
    pub payload_app_latency: Vec<Duration>,
    /// same as payload_app_latency, for streams under 1350 bytes (plain heartbeats)
    pub no_payload_app_latency: Vec<Duration>,
    /// stream frames the server sent on the unicast path, i.e., repairs of multicast data
    pub server_uc_retransmissions: Vec<ServerUcRetransmission>,
    /// fc flow congestion window over time, from the server's fc flow qlog (shared by all clients of the channel)
    pub fc_flow_cwnd: Vec<FcFlowCwnd>,
    /// fc flow rtt samples
    pub fc_flow_rtt_samples: Vec<RttSample>,
    /// multicast flow one way latency, based on the sent times on the fc flow and this receiver's received time
    /// the durations are options since the client's clock may be slightly behind the source (which would potentially result in a negative duration...)
    pub fc_flow_one_way_latency: Vec<Option<Duration>>,
}

/// ts is a timestamp in milliseconds since unix epoch
pub fn f64_epoch_to_system_time(ts: f64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs_f64(ts / 1000.0)
}

/// obtain the systemtime for a qlog event, based off of the reference time
pub fn event_system_time(ref_time: SystemTime, offset_ms: f32) -> SystemTime {
    ref_time + Duration::from_secs_f64(offset_ms as f64 / 1000.0)
}

/// compute the duration between two optional systemtime, None if either is missing, or if time b came before time a
pub fn between(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<Duration> {
    a.zip(b).and_then(|(a, b)| b.duration_since(a).ok())
}
