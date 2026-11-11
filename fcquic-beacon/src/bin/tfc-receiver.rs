use clap::Parser;
use fcquic_beacon::Heartbeat;
use log::{debug, error, info};
use quiche::flexicast::McConfig;
use sha2::{Digest as _, Sha256};
use std::{collections::HashMap, net::Ipv4Addr};
use tokio_fcquiche::{FcQuicMsg, io::receiver::TokioFcQuicRecv};

const MAX_DATAGRAM_SIZE: usize = 1350;

#[derive(Parser)]
struct Args {
    /// Activate flexicast extension.
    #[clap(long)]
    flexicast: bool,

    /// URL of the server to contact.
    url: url::Url,

    /// Multicast local IP.
    #[clap(short = 'l', long = "local", default_value = "0.0.0.0", value_parser)]
    local_ip: Ipv4Addr,

    /// Multicast packets are proxied using packet replication for this client.
    /// This argument is a trick to avoid out-of-band computation by the source
    /// of the proxies to the clients. If this value is true, instead of
    /// binding to the flexicast address given in the MC_ANNOUNCE frame, the
    /// client will listen to its own address and the port advertised by the
    /// source.
    #[clap(long = "proxy")]
    proxy_uc: bool,

    /// Sets the initial flow control limits on the receiver.
    /// If this parameter is not set, the receiver uses unlimited flow control.
    #[clap(long = "flow-control")]
    initial_flow_control: Option<u64>,

    /// Whether the client is allowed to use AMT as a fallback mechanism to
    /// remain in the multicast group when in a non native multicsat network
    #[clap(long = "amt-fallback")]
    amt_fallback_allowed: bool,

    /// AMT Relay IP
    #[clap(long = "amt-relay", value_parser)]
    amt_relay_ip: Option<Ipv4Addr>,

    /// Delay in ms without any native multicast packet before falling back to
    /// AMT. Re-armed on every native multicast packet, so it detects both a
    /// network without multicast at join time and multicast breaking
    /// mid-session.
    ///
    /// Must be well above the source's `--fc-keepalive` interval (~3x, e.g.,
    /// 750 for a 250 ms keepalive). Without a keepalive on the source, an idle
    /// flexicast flow triggers a spurious switch to AMT after this delay.
    // #[clap(long = "amt-fallback-delay", value_parser, default_value = "750")]
    // amt_fallback_delay: u64,

    // ----- FCQUIC specific arguments --------
    /// whether or not the hash of the payloads is checked
    #[clap(long = "verify-hash", default_value_t = true, action = clap::ArgAction::Set)]
    verify_hash: bool,

    /// whether or not to parse the JSON content of the hearbeats and pretty print them
    #[clap(long = "pretty-print", default_value_t = true, action = clap::ArgAction::Set)]
    pretty_print: bool,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 3)]
async fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let args = Args::parse();

    let (tx_app, rx_app) = tokio::sync::mpsc::channel(1000);

    // Create the Flexicast Quiche tokio receiver.
    let peer_addr = *args.url.socket_addrs(|| None).unwrap().first().unwrap();
    let config = get_config(&args);
    let (mut tfc_recv, rx_app) = TokioFcQuicRecv::new(
        peer_addr,
        config,
        args.local_ip,
        args.flexicast,
        args.proxy_uc,
        rx_app,
        None,
        args.amt_fallback_allowed,
        args.amt_relay_ip,
    );

    tokio::spawn(async move {
        let mut rx_app = rx_app;

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut closing = false;

        let mut up_payload_bytes: u64 = 0;
        let mut down_payload_bytes: u64 = 0;
        let mut first_data: Option<tokio::time::Instant> = None;
        let mut last_data: Option<tokio::time::Instant> = None;

        let mut streams: HashMap<u64, Vec<u8>> = HashMap::new();

        let mut batch: Vec<FcQuicMsg> = Vec::with_capacity(20);
        loop {
            tokio::select! {
                // receive as many msgs as possible from the mc flow
                // we do this to speed up the cases where a few msgs are queued up
                n = rx_app.recv_many(&mut batch, 20) => {
                     if n == 0 {
                         break; // run() is done and dropped the sender
                     }
                    //  if closing {
                    //     batch.clear(); // we're closing, so discard all these packets
                    //     continue;
                    // }

                     for msg in batch.drain(..) {
                         match msg {
                             FcQuicMsg::Close => {
                                 debug!("Receiver application received FcQuicMsg::Close");
                             },

                             FcQuicMsg::Stream((data, fin, stream_id)) => {
                                 debug!("Application received {} bytes for stream={} with fin={}", data.len(), stream_id, fin);

                                 // get the buffer for this stream from the map
                                 let map_buffer = streams.entry(stream_id).or_default();

                                 // add the received data to the buffer
                                 map_buffer.extend_from_slice(&data);

                                let n = data.len() as u64;
                                if n > 0 {
                                    let now = tokio::time::Instant::now();
                                    first_data.get_or_insert(now);
                                    last_data = Some(now);
                                    up_payload_bytes += n;
                                    down_payload_bytes += n;
                                }

                                 // only consume the data from the buffer once the entire stream has been received
                                 if !fin {
                                     continue;
                                 }

                                 // re borrow the map by removing the entry associated with this stream since it just finished.
                                 // we don't want to store all of the stream content in ram, we'll consume it here then drop it
                                 let data = streams.remove(&stream_id).unwrap();

                                 // the data is JSON header line, followed by a newline and then optionally payload bytes
                                 let header_end = data
                                     .iter()
                                     .position(|&b| b == b'\n')
                                     .unwrap_or(data.len());
                                 let heartbeat: Heartbeat = serde_json::from_slice(&data[..header_end])
                                     .expect("Failed to parse received heartbeat!");

                                 let payload_bytes = &data.get(header_end + 1..).unwrap_or(&[]);


                                 if args.pretty_print {
                                     let now_ms = chrono::Utc::now().timestamp_millis();

                                     let sent_at =
                                         chrono::DateTime::from_timestamp_millis(heartbeat.sent_at_ms as i64)
                                             .map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
                                             .unwrap_or_else(|| format!("{} ms (invalid)", heartbeat.sent_at_ms));

                                     let beacon_start = chrono::DateTime::from_timestamp_millis(
                                         heartbeat.sent_at_ms as i64 - heartbeat.uptime_s as i64 * 1000,
                                     )
                                     .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                     .unwrap_or_else(|| "invalid".to_string());

                                     let payload = match &heartbeat.payload {
                                         Some(p) => format!("{} bytes, sha256={}", p.len, hex::encode(p.hash)),
                                         None => "none".to_string(),
                                     };

                                     info!(
                                         "Heartbeat #{} from {} (session {:08x})\n  sent at:  {} ({}ms ago)\n  interval: {}ms\n  uptime:   {}s (beacon started at {})\n  payload:  {}",
                                         heartbeat.seqnum,
                                         heartbeat.beacon,
                                         heartbeat.session_id,
                                         sent_at,
                                         now_ms - heartbeat.sent_at_ms as i64,
                                         heartbeat.interval_ms,
                                         heartbeat.uptime_s,
                                         beacon_start,
                                         payload,
                                     );
                                 } else {
                                     info!(
                                         "Received heartbeat {:?} ({} bytes)",
                                         heartbeat,
                                         data.len(),
                                     );
                                 }


                                 if args.verify_hash && heartbeat.payload.is_some() {
                                     let hash_bytes = hex::encode(Sha256::digest(&payload_bytes));
                                     let correct = heartbeat.payload.is_some_and(|p| {
                                         // debug!("hex::encode(p.hash)={}", hex::encode(p.hash));
                                         // debug!("hash_bytes={hash_bytes}");
                                         hex::encode(p.hash) == hash_bytes
                                     });
                                     if correct {
                                         info!("Hash of the payload bytes matches with the hash found in heartbeat");
                                     } else {
                                         error!(
                                             "Hash of the payload bytes did not match the hash in the heartbeat, the payload bytes were not received properly."
                                         );
                                     }
                                 }
                             },
                             _ => (),
                         }
                     }
                 },

                 _ = &mut ctrl_c, if !closing => {
                     let now = chrono::Local::now();

                     println!("Closing connection at {}....", now);
                     debug!("Sending Close message {}", now);

                     if let Err(e) = tx_app.send(FcQuicMsg::Close).await {
                         debug!("failed to send Close message: {:?}", e);
                         break;
                     }

                    if let (Some(start), Some(end)) = (first_data, last_data) {
                        let duration_sec = (end - start).as_secs_f64().max(1e-9);
                        let down_mbps = (down_payload_bytes as f64) * 8.0 / duration_sec / 1_000_000.0;
                        let up_mbps = (up_payload_bytes as f64) * 8.0 / duration_sec / 1_000_000.0;

                        println!("RESULT-GOODPUT-PAYLOAD-UP-BYTES {}", up_payload_bytes);
                        println!("RESULT-GOODPUT-PAYLOAD-DOWN-BYTES {}", down_payload_bytes);
                        println!("RESULT-GOODPUT-PAYLOAD-DURATION-S {:.6}", duration_sec);
                        println!("RESULT-GOODPUT-PAYLOAD-UP-MBPS {:.3}", up_mbps);
                        println!("RESULT-GOODPUT-PAYLOAD-DOWN-MBPS {:.3}", down_mbps);
                    }

                    closing = true;

                    // allow for a second ctrl c to force quit if it hangs
                    tokio::spawn(async {
                        let _ = tokio::signal::ctrl_c().await;
                        std::process::exit(1);
                    });
                 }
            }
        }
    });

    // Start the Flexicast QUIC receiver.
    if let Err(e) = tfc_recv.run().await {
        error!("receiver errored out: {e:?}");
        std::process::exit(1);
    }

    debug!("Client disconnected.");
    println!("Client disconnected.");

    // force exit
    std::process::exit(0);
}

fn get_config(args: &Args) -> quiche::Config {
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
    config.verify_peer(false); // Not prodction-ready.

    config
        .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
        .unwrap();

    if !args.flexicast {
        config.set_max_idle_timeout(100_000);
    }
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);

    let initial_max_data = match args.initial_flow_control {
        Some(v) => v,
        None => 100_000_000_000,
    };
    config.set_initial_max_data(initial_max_data);
    config.set_initial_max_stream_data_bidi_local(initial_max_data);
    config.set_initial_max_stream_data_bidi_remote(initial_max_data);
    config.set_initial_max_stream_data_uni(initial_max_data);
    config.set_initial_max_streams_bidi(initial_max_data);
    config.set_initial_max_streams_uni(initial_max_data);
    config.set_active_connection_id_limit(10);
    config.verify_peer(false);
    config.set_cc_algorithm(quiche::CongestionControlAlgorithm::CUBIC);

    if args.flexicast {
        config.set_initial_max_path_id(10);
        config.set_enable_flexicast(args.flexicast);
        config.set_recv_fec(false);
        config.set_send_fec(false);
    }

    config
}
