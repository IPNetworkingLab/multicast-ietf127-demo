use clap::Parser;
use fcquic_beacon::Heartbeat;
use log::{debug, error, info};
use sha2::{Digest as _, Sha256};
use tokio::net::UnixDatagram;

#[derive(Parser)]
struct Args {
    /// beacon binary's socket bind path, this is akin to this binary's local address on this socket
    /// the fcquic receiver will send the filenames to this address
    #[clap(
        long = "socket-bind-path",
        value_parser,
        default_value = "/tmp/fcquic-recv.sock"
    )]
    socket_bind_path: String,

    /// whether or not the hash of the payloads is checked
    #[clap(long = "verify-hash", default_value_t = true)]
    verify_hash: bool,

    /// whether or not to parse the JSON content of the hearbeats and pretty print them
    #[clap(long = "pretty-print", default_value_t = true)]
    pretty_print: bool,
}

#[tokio::main]
async fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let args = Args::parse();

    // delete any previous binds on the receiver's bind path
    // so that the current binary can bind a socket with this address and receive from the fcquic receiver
    let _ = std::fs::remove_file(&args.socket_bind_path);
    let socket = UnixDatagram::bind(&args.socket_bind_path)
        .expect("Failed to bind a UnixDatagram socket on the socket bind path");

    info!("beacon receiver bound on the socket");

    // the buffer only holds the filenames
    let mut buffer = [0u8; 4096];

    loop {
        match socket.recv_from(buffer.as_mut_slice()).await {
            Ok((read, _from)) => {
                debug!("Read {read} from fcquic receiver");
                let filename = std::str::from_utf8(&buffer[..read]).unwrap();

                // we just received a filename over the socket, so we
                let content = std::fs::read(filename).expect("Failed to read heartbeat file");

                // the file is the JSON header line, followed by a newline and then the payload bytes
                let header_end = content
                    .iter()
                    .position(|&b| b == b'\n')
                    .unwrap_or(content.len());
                let heartbeat: Heartbeat = serde_json::from_slice(&content[..header_end])
                    .expect("Failed to parse received heartbeat!");

                if args.pretty_print {
                    
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let sent_at =
                        chrono::DateTime::from_timestamp_millis(heartbeat.sent_at_ms as i64)
                            .map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
                            .unwrap_or_else(|| format!("{} ms (invalid)", heartbeat.sent_at_ms));

                    let payload = match &heartbeat.payload {
                        Some(p) => format!("{} bytes, sha256 {}", p.len, hex::encode(p.hash)),
                        None => "none".to_string(),
                    };
                    
                    info!(
                        "Heartbeat #{} from {} (session {:08x})\n  sent at:  {} ({} ms ago)\n  interval: {} ms\n  uptime:   {} s\n  payload:  {}",
                        heartbeat.seqnum,
                        heartbeat.beacon,
                        heartbeat.session_id,
                        sent_at,
                        now_ms - heartbeat.sent_at_ms as i64,
                        heartbeat.interval_ms,
                        heartbeat.uptime_s,
                        payload,
                    );
                } else {
                    info!(
                        "Received heartbeat {:?} ({} bytes):\n{}",
                        &filename,
                        content.len(),
                        String::from_utf8_lossy(&content)
                    );
                }
            }
            Err(e) => {
                error!("Failed to read from socket: {e:?}");
            }
        }
    }
}
