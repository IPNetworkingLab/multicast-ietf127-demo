use clap::Parser;
use fcquic_beacon::{Heartbeat, Payload};
use log::{debug, error, info};
use rand::RngExt as _;
use sha2::{Digest as _, Sha256};
use std::{
    borrow::Cow,
    fs::File,
    io::{BufWriter, Write},
    time::{Duration, Instant, SystemTime},
};
use tokio::net::UnixDatagram;

// local bind path of this socket
// this is the local "address" of this binary
const UNIX_SOCK_BIND_PATH: &str = "/tmp/fcquic-src";

#[derive(Parser)]
struct Args {
    /// Path of the socket the fcquic source will read from
    /// it will create a socket and bind to it, this is the fcquic source's "address" that we'll send to
    source_path: String,

    /// output path for the hearbeats
    out_path: String,

    /// interval between each file transfer in ms
    #[clap(long = "send-interval", default_value = "500")]
    send_interval: u32,

    /// every how many heartbeats a payload is added (e.g., 10 means every 10th heartbeat)
    #[clap(long = "payload-every", default_value = "10", value_parser = clap::value_parser!(u64).range(1..))]
    payload_every: u64,

    /// size of the payload in bytes
    #[clap(long = "payload-size", default_value = "10000")]
    payload_size: u32,
}

#[tokio::main]
async fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let args = Args::parse();

    std::fs::create_dir_all(&args.out_path)
        .expect("Failed to create the heartbeat output directory");

    // remove any previous bind file
    let _ = std::fs::remove_file(UNIX_SOCK_BIND_PATH);

    // create the socket, and set this binary's local "address"
    let socket = UnixDatagram::bind(UNIX_SOCK_BIND_PATH)
        .expect("Failed to bind a UnixDatagram socket on the socket bind path");

    let beacon = Cow::from("fcquic-beacon/1");
    let session_id: u32 = rand::random();
    info!("Session ID: {session_id}");

    debug!("Will sleep for 2 seconds");
    tokio::time::sleep(Duration::from_secs(2)).await;
    debug!("Done sleeping, will start sending stuff");

    let start = Instant::now();
    let mut seqnum: u64 = 0;
    let mut bytes = vec![0u8; args.payload_size as usize];
    let mut interval = tokio::time::interval(Duration::from_millis(args.send_interval as u64));

    loop {
        let payload_len = if (seqnum % args.payload_every) == 0 {
            args.payload_size
        } else {
            0
        };
        debug!("Payload this iter: {}", payload_len);

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
            interval_ms: args.send_interval,
            uptime_s,
            payload,
        };

        debug!("Sending {}", serde_json::to_string(&heartbeat).unwrap());
        let file_path = format!("{}/hb-{}.bin", &args.out_path, seqnum);
        match write_heartbeat(&file_path, &heartbeat, payload_bytes) {
            Ok(()) => {
                info!("Serialized and wrote heartbeat and payload to file");
                let sent = socket
                    .send_to(file_path.as_bytes(), &args.source_path)
                    .await
                    .expect("Failed to send filename on the unix socket!");

                info!("Sent {sent} bytes to FCQUIC");
            }
            Err(e) => {
                error!("Failed to serialize and/or write heartbeat and payload to file: {e:?}")
            }
        }

        seqnum += 1;
        interval.tick().await;
    }
}

fn write_heartbeat(
    file_path: &str,
    heartbeat: &Heartbeat,
    payload_bytes: &mut [u8],
) -> std::io::Result<()> {
    // use a BufWriter to avoid having too many system calls
    let mut file = BufWriter::new(File::create(file_path)?);

    // write the heartbeat json first, then a newline and then the bytes
    serde_json::to_writer(&mut file, &heartbeat)?;
    file.write_all(b"\n")?;
    file.write_all(&payload_bytes)?;
    file.flush()?;

    Ok(())
}
