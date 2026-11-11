use fcquic_beacon::Heartbeat;
use fcquic_beacon::Payload;
use log::debug;
use log::info;
use quiche::fec::schedulers::FecSchedulerAlgorithm;
use quiche::flexicast::FcConfig;
use quiche::flexicast::McConfig;
use quiche::flexicast::cca::FcFlowCwnd;
use quiche::flexicast::nack::FcAckDelayStrategy;
use rand::RngExt as _;
use sha2::Digest as _;
use sha2::Sha256;
use std::borrow::Cow;
use std::net;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use tokio::sync::mpsc;
use tokio_fcquiche::FcQuicMsg;

use clap::Parser;
use tokio_fcquiche::FcFallBackDelay;
use tokio_fcquiche::io::TokioFcQuicConfig;

#[derive(Parser)]
struct Args {
    /// Activate flexicast extension.
    #[clap(long)]
    flexicast: bool,

    /// Keylog file for flexicast channel.
    #[clap(long = "keylog", value_parser, default_value = "./fc-flow-keylog.txt")]
    fc_keylog_file: String,

    /// Source address of the server.
    #[clap(long = "src", default_value = "127.0.0.1:4433")]
    src_addr: net::SocketAddr,

    /// Certificate path.
    #[clap(long = "cert-path", value_parser, default_value = "./src/bin")]
    cert_path: String,

    /// Multicast address.
    #[clap(long = "mc-addr", value_parser, default_value = "239.239.239.35:4434")]
    mc_addr: net::SocketAddr,

    /// Multicast source address.
    /// Must be different from the source address.
    #[clap(long = "mc-src-addr", value_parser, default_value = "127.0.0.1:4567")]
    mc_src_addr: net::SocketAddr,

    /// Flexicast flow timer.
    #[clap(long = "fc-timer", value_parser, default_value = "0")]
    fc_ack_delay: FcAckDelayStrategy,

    /// Specify the congestion window for the flexicast flow.
    /// The possible values are:
    /// - A string value representing a congestion control algorithm;
    /// - An integer value representing the fixed congestion window;
    /// - The 'disabled' string, representing an unlimited congestion window.
    #[clap(long = "fc-cwnd", default_value = "cubic")]
    fc_cwnd: FcFlowCwnd,

    /// RTP message to indicate the end of the stream.
    #[clap(long = "rtp-stop", value_parser, default_value = "STOP RTP")]
    rtp_stop: String,

    /// Number of clients to listen before actually sending data to the wire.
    #[clap(long = "wait", value_parser)]
    wait: Option<u64>,

    /// Whether the application allows unicast delivery instead of flexicast.
    #[clap(long = "unicast")]
    allow_unicast: bool,

    /// Whether the unicast path has unlimited congestion window.
    #[clap(long = "unicast-unlimited-cwnd")]
    uc_unlimited_cwnd: bool,

    /// Whether the flexicast flow must be created using path probing.
    #[clap(long = "probe-path")]
    probe_path: bool,

    /// Unicast fall-back delay for the scheduler.
    /// Either a static value in ms (e.g. 500) or 'adaptive'.
    #[clap(long = "fall-back-delay", value_parser)]
    fall_back_delay: Option<FcFallBackDelay>,

    /// Whether to use `sendmmsg` instead of relying on real flexicast
    /// to distribute data on the flexicast flow.
    /// The value is the number of instances of sendmmsg to use.
    /// For now, creates 'sendmmsg' instances for each flexicast flow.
    #[clap(long = "sendmmsg", value_parser)]
    sendmmsg: Option<u64>,

    /// Sets the delay (in ms) before sending aggregated acknowledgments to the
    /// flexicast flow for the controller. Used to avoid saturating with
    /// messages the flexicast flow source.
    #[clap(long = "ctl-ack-delay")]
    ctl_ack_delay: Option<u64>,

    /// Sets the initial flow control for the flexicast flow.
    #[clap(long = "initial-fc-flow")]
    initial_fc_flow: Option<u64>,

    /// Whether to use FEC for flexicast.
    /// If set, defines the FEC scheduler to use.
    #[clap(long = "fec-scheduler")]
    fec_scheduler: Option<FecSchedulerAlgorithm>,

    /// Number of leaf controllers to use.
    #[clap(long = "nb-controllers", default_value = "1")]
    nb_controllers: u64,

    /// Minimum bandwidth gain ratio to trigger unicast fallback for the
    /// slowest receiver. The slowest receiver is ejected when removing it
    /// would multiply the group's bottleneck rate by at least this factor.
    /// If not set, the controller never auto-ejects slow receivers.
    #[clap(long = "fallback-gain-ratio")]
    fallback_gain_ratio: Option<f64>,

    /// Minimum number of delivery-rate samples before a receiver is eligible
    /// for auto fallback. If not set, defaults to the compiled-in constant.
    #[clap(long = "fallback-min-samples")]
    fallback_min_samples: Option<u64>,

    /// Seconds between reintegration eligibility checks for fallen-back
    /// receivers. If not set, fallen-back receivers are never reintegrated.
    #[clap(long = "reintegration-delay")]
    reintegration_initial_delay: Option<f64>,

    /// Maximum expected acknowledgment rate, in bps.
    #[clap(long = "max-ack-rate", default_value = "100000000")]
    max_ack_rate: u64,

    /// Time between two ack delay updates.
    #[clap(long = "ack-delay-frame", default_value = "100")]
    ack_delay_latency: u64,

    /// Interval for non ack eliciting pings to be sent on the multicast flow in
    /// milliseconds. This frame will be sent on the mc flow every interval,
    /// except if the application is active, in which case the pings will be
    /// suppressed. This is usefule to make sure that the FC flow is seen as
    /// alive by receivers (and is used to that effect to enable AMT fallback)
    #[clap(long = "fc-ping-interval")]
    fc_ping_interval: u64,

    /// Number of new clients that will be asked to ACK a certain number of
    /// FC_PING frame. These frames go out when the multicast flow is idle.
    /// Example: fc-ping-new-clients-acks = 10, fc-ping-acks-per-new-client = 3,
    /// so new clients will, in groups of 10, be asked to ACK 3 FC_PING frames
    /// each
    #[clap(long = "fc-ping-new-clients-acks", default_value = "250")]
    fc_ping_max_new_clients: u64,

    /// Number of ACKs to FC_PING frames that each new client will be asked to
    /// send.
    #[clap(long = "fc-ping-acks-per-new-client", default_value = "30")]
    fc_ping_acks_per_new_client: usize,

    // -------- FCQUIC BEACON Specific ---------
    /// interval between each file transfer in ms
    #[clap(long = "send-interval", default_value = "500")]
    send_interval: u32,

    /// every how many heartbeats a payload is added (e.g., 10 means every 10th heartbeat)
    #[clap(long = "payload-every", default_value = "10", value_parser = clap::value_parser!(u32).range(1..))]
    payload_every: u32,

    /// size of the payload in bytes
    #[clap(long = "payload-size", default_value = "10000")]
    payload_size: u32,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let args = Args::parse();

    info!("src: {:?}", args.src_addr);
    info!("mc-addr: {:?}", args.mc_addr);
    info!("mc-src-addr: {:?}", args.mc_src_addr);

    // Create Flexicast Quiche tokio config.
    let fc_quic_tokio_config = TokioFcQuicConfig {
        unicast: args.allow_unicast,
        unicast_unlimited_cwnd: args.uc_unlimited_cwnd,
        sendmmsg: args.sendmmsg,
        wait: args.wait,
        flexicast: args.flexicast,
        fc_keylog_file: args.fc_keylog_file.clone(),
        fallback_delay: args.fall_back_delay.clone(),
        uc_src_addr: args.src_addr,
        nb_leaf_controllers: args.nb_controllers,
        h3_config: None,
        max_ack_rate: args.max_ack_rate,
        fallback_gain_ratio: args.fallback_gain_ratio,
        fallback_min_samples: args.fallback_min_samples,
        reintegration_initial_delay: args
            .reintegration_initial_delay
            .map(std::time::Duration::from_secs_f64),
    };

    // Transmission channel towards the application, supposed to be unique
    // because the receivers may send messages without knowing to which
    // flexicast flow it belongs.
    let (tx_app, rx_app) = tokio::sync::mpsc::channel(100);

    let mut fcquiche = tokio_fcquiche::io::TokioFcQuic::new(fc_quic_tokio_config, tx_app);
    // Create a single flexicast flow.
    let flow_config = FcConfig {
        fc_tp: args.flexicast,
        probe_mc_path: false,
        max_data: args.initial_fc_flow.unwrap_or(100_000_000_000),
        max_stream_data: args.initial_fc_flow.unwrap_or(100_000_000_000),
        fc_ack_delay: args.fc_ack_delay,
        fec: args.fec_scheduler.is_some(),
        fec_scheduler: args
            .fec_scheduler
            .unwrap_or(FecSchedulerAlgorithm::NoRedundancy),
        src_addr: args.mc_src_addr,
        mc_addr: args.mc_addr,
        crt_path: args.cert_path.clone(),
        fc_cca: args.fc_cwnd,
        ack_delay_latency: std::time::Duration::from_millis(args.ack_delay_latency),
        fc_ping_interval: std::time::Duration::from_millis(args.fc_ping_interval),
        fc_ping_max_new_clients: args.fc_ping_max_new_clients,
        fc_ping_acks_per_new_client: args.fc_ping_acks_per_new_client,
        ..Default::default()
    };

    fcquiche
        .add_fc_flow(flow_config, &args.fc_keylog_file)
        .await
        .unwrap();

    // Get the transmission channel to send application content.
    let tx_fc = fcquiche.get_tx_fc_flow(0).unwrap();

    tokio::spawn(async move {
        beacon_loop(
            args.send_interval,
            args.payload_size,
            args.payload_every,
            tx_fc,
            rx_app,
        )
        .await;
    });

    // Start Tokio Flexicast Quiche.
    let uc_config = get_config(&args);

    fcquiche.run(uc_config).await.unwrap();
}

async fn beacon_loop(
    send_interval: u32,
    payload_size: u32,
    payload_every: u32,
    tx_fc: mpsc::Sender<FcQuicMsg>,
    mut rx_app: mpsc::Receiver<FcQuicMsg>,
) {
    let beacon = Cow::from("fcquic-beacon/1");
    let session_id: u32 = rand::random();
    info!("Session ID: {session_id}");

    let mut seqnum: u64 = 0;
    let mut bytes = vec![0u8; payload_size as usize];

    // the source will send a beacon based on the period below
    let period = Duration::from_millis(send_interval as u64);
    // but for the first period, we start it in 5 seconds
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_secs(2), period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let start = Instant::now();

    // The stream id for the flexicast flow starts as 0x3 because it has "0b11" as the least significant bits
    // This tells quic that the stream is a unidirectional server-initiated stream
    // We'll then increment by 4 in order to get the next number with those same LSBs.
    let mut stream_id = 3;

    loop {
        tokio::select! {
            msg = rx_app.recv() => {
                match msg {
                    None | Some(FcQuicMsg::Close) => {
                        break;
                    },
                    Some(FcQuicMsg::Stream((data, fin, stream_id))) => {
                        info!("Received stream from receiver!: {:?}", FcQuicMsg::Stream((data, fin, stream_id)));
                    },
                    Some(_) => (),
                }
            },
            _ = interval.tick() => {

                // we'll send a payload of payload_len if this iteration is a multiple of payload_every
                // (e.g., if this is iter n° 20, and we send a payload every 10 iters)
                let payload_len = if (seqnum % (payload_every as u64)) == 0 {
                    payload_size
                } else {
                    0
                };
                debug!("Payload this iter: {} bytes", payload_len);

                let now_ts = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Failed to compute EPOCH timestamp!")
                    .as_millis() as u64;

                let uptime_s = Instant::now().duration_since(start).as_secs() as u32;

                let payload_bytes = &mut bytes[..payload_len as usize];
                rand::rng().fill(payload_bytes);

                let payload = if payload_len > 0 {
                    let val = Payload {
                        len: payload_len,
                        hash: Sha256::digest(&payload_bytes).into(),
                    };
                    Some(val)
                } else {
                    None
                };

                let heartbeat = Heartbeat {
                    beacon: beacon.clone(),
                    session_id,
                    seqnum,
                    sent_at_ms: now_ts,
                    interval_ms: send_interval,
                    uptime_s,
                    payload,
                };

                info!("Sending {}", serde_json::to_string(&heartbeat).expect("Failed to serialize the heartbeat into a string"));
                let mut data = serde_json::to_vec(&heartbeat).expect("Failed to serialize the heartbeat!");
                // then push the payload bytes to the vec
                data.push(b'\n');
                data.extend_from_slice(payload_bytes);

                let fc_msg = FcQuicMsg::Stream((data, true, stream_id));

                // increment the stream id by 4 to keep it as a server initiated unidirectional stream
                stream_id += 4;

                if let Err(e) = tx_fc.send(fc_msg).await {
                    log::error!("Failed to send message to flexicast flow: {:?}", e);
                    continue;
                }

                seqnum+=1;
            }
        }
    }
}

fn get_config(args: &Args) -> quiche::Config {
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    config
        .load_cert_chain_from_pem_file(
            Path::new(&args.cert_path)
                .join("cert.crt")
                .to_str()
                .unwrap(),
        )
        .unwrap();
    config
        .load_priv_key_from_pem_file(
            Path::new(&args.cert_path)
                .join("cert.key")
                .to_str()
                .unwrap(),
        )
        .unwrap();

    config
        .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
        .unwrap();

    config.set_max_recv_udp_payload_size(tokio_fcquiche::MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(tokio_fcquiche::MAX_DATAGRAM_SIZE);

    let initial_max_data = match args.initial_fc_flow {
        Some(v) => v,
        None => 100_000_000_000,
    };
    config.set_initial_max_data(initial_max_data);
    config.set_initial_max_stream_data_bidi_local(initial_max_data);
    config.set_initial_max_stream_data_bidi_remote(initial_max_data);
    config.set_initial_max_stream_data_uni(initial_max_data);
    config.set_initial_max_streams_bidi(initial_max_data);
    config.set_initial_max_streams_uni(initial_max_data);
    config.set_disable_active_migration(true);
    config.set_active_connection_id_limit(5);
    config.enable_early_data();
    config.enable_pacing(false);
    config.set_send_fec(false);
    config.set_recv_fec(false);
    config.set_enable_flexicast(args.flexicast);
    config.set_initial_max_path_id(10);
    config.set_cc_algorithm(quiche::CongestionControlAlgorithm::CUBIC);

    config
}
