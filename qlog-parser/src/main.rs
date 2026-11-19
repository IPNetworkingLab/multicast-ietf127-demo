use clap::Parser;
use log::{debug, error, info, trace};
use qlog_parser::{
    AmtTunnelUpdate, ClientStateUpdate, ConnMetrics, FallBackUpdate, FcFlowCwnd, FcFlowOneWayDelay,
    ReceivedStream, ReceptionModeUpdate, RetransmissionEvent, RttSample, ServerUcRetransmission,
    StartupPhases, between, event_system_time, f64_epoch_to_system_time,
};
use rayon::prelude::*;
use regex::Regex;
use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
};

#[derive(Parser)]
struct Args {
    /// output path for the json files
    out_path: String,

    #[clap(long = "in-path", default_value = "./qlogs")]
    in_path: String,

    /// Optional: specify a client's DCID to only parse that client connection
    #[clap(long = "dcid")]
    specific_dcid: Option<String>,
}

#[derive(Debug)]
struct ClientLog {
    client_log_filename: Option<PathBuf>,
    server_log_filename: Option<PathBuf>,
}

/// for the qlog dir, find the unicast path qlog files belonging to the same connection from the pov of the client and the source, then also
/// store all of the multicast flow qlogs files in a map that with the client uc path channel id as the key, and the filename as the value
fn read_qlog_dir(qlog_dir: &Path) -> (HashMap<String, ClientLog>, HashMap<String, PathBuf>) {
    let re = Regex::new(r"^(client|server-server)-.*-([0-9a-f]+)\.sqlog(?:\.gz)?$").unwrap();
    let fc_flow_qlogs_re = Regex::new(r"^server-fc-flow-.*-([0-9a-f]+)\.sqlog(?:\.gz)?$").unwrap();

    let mut logs_map = HashMap::new();
    let mut fc_flow_qlogs_map = HashMap::new();

    for entry in qlog_dir
        .read_dir()
        .expect("Failed to read qlog dir")
        .flatten()
    {
        // if let Ok(entry) = entry {
        let filename = entry.file_name();
        let log_name = filename.to_str().unwrap();

        // unicast qlogs
        if let Some(c) = re.captures(log_name) {
            let (role, dcid) = (&c[1], &c[2]);

            debug!("role={role}, dcid={dcid}");

            let map_entry = logs_map.entry(dcid.to_string()).or_insert(ClientLog {
                client_log_filename: None,
                server_log_filename: None,
            });
            match role {
                "client" => {
                    map_entry.client_log_filename = Some(entry.path());
                }
                "server-server" => {
                    map_entry.server_log_filename = Some(entry.path());
                }
                _ => (),
            }
        }

        // mc flow qlog
        if let Some(capture) = fc_flow_qlogs_re.captures(log_name) {
            let channel_id = &capture[1];
            debug!(
                "Multicast flow qlog file '{log_name}' for client with channel_id={channel_id:?}"
            );
            fc_flow_qlogs_map.insert(channel_id.to_string(), entry.path());
        }
    }
    // }
    (logs_map, fc_flow_qlogs_map)
}

/// Check for a range of bytes received for a specific stream if the newly received frame (based on the offset and length) is duplicated or not
/// uc_in_mc_mode is Some(true) if the frame is a retransmitted frame that originally was sent over multicast, it's None whenever the from_mc field isn't present in the qlogs
/// if it's None, we need to try to figure out based on the packet number if the frame was sent on unicast or multicast. For this, we use a simple heuristic that is based on
/// the fact that the multicast flow's packet number space grows much much faster compared to the uc path's pn space, so if the packets for a stream go from a large number to a much smaller number,
/// we lost a frame from the mc flow and received a retransmission over unicast
fn record_stream_frame(
    stream: &mut ReceivedStream,
    pn: Option<u64>,
    uc_in_mc_mode: Option<bool>,
    offset: u64,
    length: u64,
) -> Option<bool> {
    let ranges = &mut stream.ranges;
    let end = offset + length;

    // ranges are kept merged and disjoint, so the last one ends at the highest byte seen

    // if the highest value (i.e., highest end offset) that we've already received is larger than the new offset,
    // then this frame's content was already received -> it was duplicated
    let below_highest = offset
        < ranges
            .last_key_value()
            .map_or(0, |(_start, &curr_end)| curr_end);

    // if the current packet number for this frame for the stream is lower than the previously seen maximum packet number for the same stream
    // then this frame is a retransmitted frame over unicast -> this heuristic may not hold at the very start of the source's run
    let lower_pn = pn.zip(stream.max_pn).is_some_and(|(pn, max)| pn < max); // if pn and stream.max_pn are some, then compare them, otherwise don't compare
    stream.max_pn = stream.max_pn.max(pn); // set the max pn for this stream to pn only if it's larger than the prev max pn, i.e., pn wasn't a retransmitted frame over unicast

    // the from_mc field wasn't previously in the in the packet_received events from the qlogs of early testers, so we rely on the heuristic above to find
    // retranmsissions from those old qlogs, and use the from_mc field for the newer
    let retransmitted = below_highest || uc_in_mc_mode.unwrap_or(lower_pn);

    // the frame is duplicated if we've already seen the bytes of that range
    let duplicate = ranges
        .range(..=offset)
        .next_back()
        .is_some_and(|(_, &curr_end)| curr_end >= end);

    // insert [offset, end) and merge it with any ranges it touches
    let (mut curr_start, mut curr_end) = (offset, end);
    if let Some((&prev_start, &prev_end)) = ranges.range(..=curr_start).next_back()
        && prev_end >= curr_start
    {
        curr_start = prev_start;
        curr_end = curr_end.max(prev_end);
    }
    let touched: Vec<u64> = ranges
        .range(curr_start..=curr_end)
        .map(|(&k, _)| k)
        .collect();
    for k in touched {
        curr_end = curr_end.max(ranges.remove(&k).unwrap());
    }
    ranges.insert(curr_start, curr_end);

    retransmitted.then_some(duplicate)
}

fn parse_server_uc_events(
    server_qlog_reader: qlog::reader::QlogSeqReader<'_>,
    server_ref_time: SystemTime,
    is_persistent_client: &mut bool,
    server_acked_mc_pns: &mut BTreeSet<u64>,
    server_pov_client_state_updates: &mut Vec<ClientStateUpdate>,
    fallback_updates: &mut Vec<FallBackUpdate>,
    server_uc_retransmissions: &mut Vec<ServerUcRetransmission>,
) -> Option<std::net::Ipv4Addr> {
    let mut client_src_ip: Option<std::net::Ipv4Addr> = None;

    for event in server_qlog_reader {
        match event {
            qlog::reader::Event::Qlog(qlog_event) => {
                trace!("server event: {qlog_event:?}");
                match qlog_event.data {
                    qlog::events::EventData::ConnectionStarted(connection_started) => {
                        client_src_ip.get_or_insert(
                            std::net::Ipv4Addr::from_str(connection_started.dst_ip.as_str())
                                .expect("Failed to parse ipv4 addr from connection_started.src_ip"),
                        );

                        // there is always one client connected to make sure that the multicast flow's congestion window doesn't fill up
                        // and force the src to stop sending.
                        // the client is running on a dev vm with the ip 130.104.229.33
                        if connection_started.dst_ip.eq("130.104.229.33") {
                            *is_persistent_client = true;
                        }
                    }

                    qlog::events::EventData::PacketReceived(packet_received) => {
                        for frame in packet_received.frames.iter().flatten() {
                            // the client acks the fc flow (path 1) over unicast, the acked ranges are the mc packets it received
                            if let qlog::events::quic::QuicFrame::PathAck {
                                path_identifier: 1,
                                acked_ranges: Some(ranges),
                                ..
                            } = frame
                            {
                                let ranges: Vec<(u64, u64)> = match ranges {
                                    // a range is either [start, end] or [pn]
                                    qlog::events::quic::AckedRanges::Single(r) => r
                                        .iter()
                                        .filter_map(|r| Some((*r.first()?, *r.last()?)))
                                        .collect(),
                                    qlog::events::quic::AckedRanges::Double(r) => r.clone(),
                                };
                                for (start, end) in ranges {
                                    server_acked_mc_pns.extend(start..=end);
                                }
                            }
                        }
                    }
                    qlog::events::EventData::PacketSent(packet_sent) => {
                        for frame in packet_sent.frames.iter().flatten() {
                            // any stream frame sent over the unicast path is a retransmission, so count it as such
                            if let qlog::events::quic::QuicFrame::Stream {
                                stream_id, length, ..
                            } = frame
                            {
                                server_uc_retransmissions.push(ServerUcRetransmission {
                                    time: event_system_time(server_ref_time, qlog_event.time),
                                    stream_id: *stream_id,
                                    length: *length,
                                });
                            }
                        }
                    }
                    qlog::events::EventData::FlexicastClientStateUpdated(client_state_updated) => {
                        server_pov_client_state_updates.push(ClientStateUpdate {
                            time: event_system_time(server_ref_time, qlog_event.time),
                            channel_id: client_state_updated.channel_id,
                            client_id: client_state_updated.client_id,
                            old: client_state_updated.old,
                            new: client_state_updated.new,
                        });
                    }
                    qlog::events::EventData::FlexicastFallbackUpdated(fallback_updated) => {
                        fallback_updates.push(FallBackUpdate {
                            time: event_system_time(server_ref_time, qlog_event.time),
                            client_id: fallback_updated.client_id,
                            flow_id: fallback_updated.flow_id,
                            action: fallback_updated.action,
                            trigger: fallback_updated.trigger,
                            receiver_rtt: fallback_updated.receiver_rtt,
                            reference_rtt: fallback_updated.reference_rtt,
                            gain_ratio: fallback_updated.gain_ratio,
                        });
                    }
                    _ => (),
                };
            }
            qlog::reader::Event::Json(json_event) => {
                debug!("json_event={json_event:?}");
            }
        }
    }
    client_src_ip
}

fn parse_client_conn(
    client_file: File,
    server_file: File,
    fc_flow_qlogs_map: &HashMap<String, PathBuf>,
) -> ConnMetrics {
    // get a bufreader from the file, then wrap it in the Gzdecoder to get a decompressed reader into the qlogs
    // and then take that as a buf reader to pass to the QlogSecReader
    let client_decoded =
        BufReader::new(flate2::bufread::GzDecoder::new(BufReader::new(client_file)));
    let server_decoded = BufReader::new(server_file);

    // read the qlog file with the qlogseqreader from multicast-quic, and then process each event one by one
    let client_qlog_reader = qlog::reader::QlogSeqReader::new(Box::new(client_decoded)).unwrap();
    let server_qlog_reader = qlog::reader::QlogSeqReader::new(Box::new(server_decoded)).unwrap();

    // parse the ref times for the qlogs, this way we can compute the actual time of any event
    let client_ref_time = f64_epoch_to_system_time(
        client_qlog_reader
            .qlog
            .trace
            .common_fields
            .as_ref()
            .map(|com| com.reference_time.unwrap())
            .expect("Failed to obtain client_ref_time"),
    );
    debug!("Client ref time: {client_ref_time:?}");

    let server_ref_time = f64_epoch_to_system_time(
        server_qlog_reader
            .qlog
            .trace
            .common_fields
            .as_ref()
            .map(|com| com.reference_time.unwrap())
            .expect("Failed to obtain server_ref_time"),
    );
    debug!("Server ref time: {server_ref_time:?}");

    let mut client_channel_id: Option<String> = None;
    let mut fc_flow_qlog_reader: Option<qlog::reader::QlogSeqReader> = None;

    let mut is_persistent_client = false;
    let mut client_src_ip: Option<std::net::Ipv4Addr> = None;

    let mut open_time: Option<SystemTime> = None;
    let mut closed_reason: Option<String> = None;
    let mut closed_time: Option<SystemTime> = None;
    let mut rtt: Vec<RttSample> = Vec::with_capacity(10);
    let mut reception_mode_updates: Vec<ReceptionModeUpdate> = Vec::with_capacity(5);
    let mut amt_tunnel_updates: Vec<AmtTunnelUpdate> = Vec::with_capacity(10);
    let mut client_state_updates: Vec<ClientStateUpdate> = Vec::with_capacity(20);
    let mut server_pov_client_state_updates: Vec<ClientStateUpdate> = Vec::with_capacity(20);
    let mut fallback_updates: Vec<FallBackUpdate> = Vec::with_capacity(0); // the initial capacity is zero because fallbacks don't typically happen, the amt fallback events are not represented in here

    // retransmission detection state
    // Maps stream ids to the merged received ranges (start, end)
    let mut streams: HashMap<u64, ReceivedStream> = HashMap::new();
    let mut retransmission_events: Vec<RetransmissionEvent> = Vec::new();

    let mut received_bytes = 0u64;
    let mut sent_bytes = 0u64;

    let mut last_mcast_pn: Option<u64> = None;
    let mut mcast_losses = 0u64;
    let mut total_mcast_packet = 0u64;

    // fc flow packet numbers acked by the client, from the server's qlog
    let mut server_acked_mc_pns: BTreeSet<u64> = BTreeSet::new();

    let mut mc_join_time: Option<SystemTime> = None;
    let mut amt_opening_time: Option<SystemTime> = None;
    let mut amt_opened_time: Option<SystemTime> = None;
    let mut first_pkt_recvd_time: Option<SystemTime> = None;
    let mut first_stream_data_time: Option<SystemTime> = None;

    let mut application_data_latency: Vec<(u64, Duration)> = Vec::with_capacity(100);

    let mut announced_fc_ping_interval_ms = 250;

    let mut mc_recv_by_pn: HashMap<u64, SystemTime> = HashMap::new();

    // received time of the multicast packets on the fc flow
    let mut mc_packet_received_times: Vec<SystemTime> = Vec::with_capacity(500);

    // one way mc packet delay, is an option if the client's clock is not properly synced with the source, it might be behind slightly,...
    let mut mc_one_way_delays: Vec<FcFlowOneWayDelay> = Vec::with_capacity(500);

    // vec of retransmissions sent by the server on the unicast path, each with the time, stream_id, and the size of the stream frame
    let mut server_uc_retransmissions: Vec<ServerUcRetransmission> = Vec::with_capacity(5);
    let mut fc_flow_cwnd: Vec<FcFlowCwnd> = Vec::with_capacity(1000);

    let mut fc_flow_rtt_samples: Vec<RttSample> = Vec::with_capacity(1000);

    let (server_client_ip, ()) = rayon::join(
        || {
            parse_server_uc_events(
                server_qlog_reader,
                server_ref_time,
                &mut is_persistent_client,
                &mut server_acked_mc_pns,
                &mut server_pov_client_state_updates,
                &mut fallback_updates,
                &mut server_uc_retransmissions,
            )
        },
        || {
            for event in client_qlog_reader {
                match event {
                    qlog::reader::Event::Qlog(qlog_event) => {
                        trace!("client event: {qlog_event:?}");
                        match qlog_event.data {
                            qlog::events::EventData::ConnectionStarted(connection_started) => {
                                open_time =
                                    Some(event_system_time(client_ref_time, qlog_event.time));

                                // get the source ip
                                if connection_started.src_ip != "0.0.0.0" {
                                    debug!(
                                        "Found non zero client src_ip: {}",
                                        connection_started.src_ip
                                    );

                                    client_src_ip = Some(
                                        std::net::Ipv4Addr::from_str(connection_started.src_ip.as_str())
                                            .expect(
                                                "Failed to parse ipv4 addr from connection_started.src_ip",
                                            ),
                                    );
                                }
                            }
                            qlog::events::EventData::ConnectionClosed(connection_closed) => {
                                // when the connection closes, store the close time and the reason (i think it'll always be "bye")
                                closed_time =
                                    Some(event_system_time(client_ref_time, qlog_event.time));
                                closed_reason = connection_closed.reason;
                            }
                            qlog::events::EventData::PacketReceived(packet_received) => {
                                let event_time =
                                    event_system_time(client_ref_time, qlog_event.time);

                                received_bytes += packet_received
                                    .raw
                                    .as_ref()
                                    .and_then(|r| r.length)
                                    .unwrap_or(0);

                                // if the from_mc field is present in the qlog events, then we can use it to know if this packet was sent over multicast
                                // in which case it wasn't a retransmission (since we don't retransmit over the mc flow yet)
                                let from_mc =
                                    qlog_event.ex_data.get("from_mc").and_then(|v| v.as_bool());

                                let pn = packet_received.header.packet_number;

                                if let (Some(true), Some(packet_num)) = (from_mc, pn) {
                                    total_mcast_packet += 1;

                                    mc_packet_received_times.push(event_time);
                                    // also store the time at which this packet was received, this is used to the compute the one way delay for every mc flow packet
                                    mc_recv_by_pn.insert(packet_num, event_time);

                                    match last_mcast_pn {
                                        // count a packet as lost if the current packet number is greater than the currently last_mcast packet received (+1 is implied)
                                        Some(last) if packet_num > last => {
                                            mcast_losses += packet_num - last - 1
                                        }
                                        // remove a packet that was reordered if we've received it after counting it as lost
                                        Some(last) if packet_num < last => {
                                            mcast_losses = mcast_losses.saturating_sub(1)
                                        }
                                        _ => (),
                                    }

                                    last_mcast_pn = last_mcast_pn.max(Some(packet_num));
                                }

                                for frame in packet_received.frames.iter().flatten() {
                                    // store the announced fc ping interval in order to detect fc ping gaps
                                    if let qlog::events::quic::QuicFrame::McAnnounce {
                                        fc_ping_interval,
                                        channel_id,
                                        ..
                                    } = frame
                                    {
                                        announced_fc_ping_interval_ms = *fc_ping_interval;

                                        client_channel_id.get_or_insert(channel_id.to_owned());

                                        // we now have the client channel id, so we can find the server's fc flow qlog file corresponding to this client
                                        if let Some(filename) = fc_flow_qlogs_map.get(channel_id) {
                                            let fc_flow_file = File::open(filename).expect(
                                                "Failed to open fc flow qlog file for client",
                                            );

                                            let fc_flow_decoded = BufReader::new(fc_flow_file);

                                            fc_flow_qlog_reader.get_or_insert(
                                                qlog::reader::QlogSeqReader::new(Box::new(
                                                    fc_flow_decoded,
                                                ))
                                                .unwrap(),
                                            );
                                        }
                                    }

                                    // if let qlog::events::quic::QuicFrame::FcPing { .. } = *frame {
                                    //     fc_ping_times.push(event_time);
                                    // }

                                    // destructure the stream frames
                                    if let qlog::events::quic::QuicFrame::Stream {
                                        stream_id,
                                        offset,
                                        length,
                                        fin,
                                        ..
                                    } = *frame
                                    {
                                        let stream_frame_time =
                                            event_system_time(client_ref_time, qlog_event.time);

                                        first_stream_data_time.get_or_insert(stream_frame_time);

                                        let stream = streams.entry(stream_id).or_default();
                                        stream.first_frame_time.get_or_insert(stream_frame_time);
                                        if fin == Some(true) {
                                            stream.final_size = Some(offset + length);
                                        }
                                        let pn = packet_received.header.packet_number;

                                        // if this receiver has fallenback to unicast, then receiving stream data over unicast is not a retransmission
                                        let mc_mode = reception_mode_updates.last().is_some_and(
                                            |reception_update| {
                                                !matches!(
                                                    reception_update.new_mode,
                                                    qlog::events::flexicast::ReceptionMode::Unicast
                                                )
                                            },
                                        );
                                        let uc_in_mc_mode =
                                            from_mc.map(|from_mc| !from_mc && mc_mode);

                                        if let Some(duplicate) = record_stream_frame(
                                            stream,
                                            pn,
                                            uc_in_mc_mode,
                                            offset,
                                            length,
                                        ) {
                                            retransmission_events.push(RetransmissionEvent {
                                                time: event_system_time(
                                                    client_ref_time,
                                                    qlog_event.time,
                                                ),
                                                packet_number: pn,
                                                from_mc,
                                                stream_id,
                                                offset,
                                                length,
                                                duplicate,
                                            });
                                        }
                                    }
                                }
                            }
                            qlog::events::EventData::DataMoved(data_moved) => {
                                // a stream is fully delivered once the application has read up to its final size,
                                // take() it so that each stream is only counted once
                                if let (Some(stream_id), Some(offset), Some(length)) =
                                    (data_moved.stream_id, data_moved.offset, data_moved.length)
                                    && let Some(stream) = streams.get_mut(&stream_id)
                                    && let Some(size) =
                                        stream.final_size.take_if(|size| offset + length >= *size)
                                    && let Some(latency) = between(
                                        stream.first_frame_time,
                                        Some(event_system_time(client_ref_time, qlog_event.time)),
                                    )
                                {
                                    application_data_latency.push((size, latency));
                                }
                            }
                            qlog::events::EventData::PacketSent(packet_sent) => {
                                sent_bytes +=
                                    packet_sent.raw.as_ref().and_then(|r| r.length).unwrap_or(0);
                            }
                            qlog::events::EventData::MetricsUpdated(metrics_updated) => {
                                // some metrics_updated events may only contain the bytes in flight,... but if latest_rtt is present, then smoothed_rtt will be as well
                                if let Some(latest_ms) = metrics_updated.latest_rtt {
                                    rtt.push(RttSample {
                                        time: event_system_time(client_ref_time, qlog_event.time),
                                        latest_ms,
                                        smoothed_ms: metrics_updated.smoothed_rtt,
                                    });
                                }
                            }
                            qlog::events::EventData::FlexicastReceptionModeUpdated(
                                reception_mode_updated,
                            ) => {
                                // if the client just joined (no mc_join_time set yet) and the new reception mode is multicast, then set the mc join time as now
                                if reception_mode_updated.new
                                    == qlog::events::flexicast::ReceptionMode::Multicast
                                    && mc_join_time.is_none()
                                {
                                    mc_join_time =
                                        Some(event_system_time(client_ref_time, qlog_event.time));
                                }

                                if reception_mode_updated.old
                                    == Some(qlog::events::flexicast::ReceptionMode::Unicast)
                                {
                                    // went from unicast to mcast or amt -> reset the last_mcast_pn to avoid counting all of the packets sent on the mc flow in the mean time as lost
                                    last_mcast_pn = None;
                                }

                                reception_mode_updates.push(ReceptionModeUpdate {
                                    time: event_system_time(client_ref_time, qlog_event.time),
                                    old_mode: reception_mode_updated.old,
                                    new_mode: reception_mode_updated.new,
                                    trigger: reception_mode_updated.trigger,
                                });
                            }
                            qlog::events::EventData::FlexicastAmtTunnelUpdated(
                                amt_tunnel_updated,
                            ) => {
                                let event_time =
                                    event_system_time(client_ref_time, qlog_event.time);

                                // if the client was not previously using amt, but is now opening the tunnel, log the opening time as now
                                if amt_tunnel_updated.old.is_none()
                                    && amt_tunnel_updated.new
                                        == qlog::events::flexicast::AmtTunnelState::Opening
                                {
                                    amt_opening_time = Some(event_time);
                                }

                                // if the client was previously opening the tunnel and it is now opened, log the opened time as now
                                if amt_tunnel_updated.old
                                    == Some(qlog::events::flexicast::AmtTunnelState::Opening)
                                    && amt_tunnel_updated.new
                                        == qlog::events::flexicast::AmtTunnelState::Opened
                                    && amt_opened_time.is_none()
                                {
                                    amt_opened_time = Some(event_time);
                                }

                                // if the tunnel was opened and we just received the first amt packet over it, log this time as now
                                if amt_tunnel_updated.old
                                    == Some(qlog::events::flexicast::AmtTunnelState::Opened)
                                    && amt_tunnel_updated.new
                                        == qlog::events::flexicast::AmtTunnelState::FirstPacketReceived
                                    && first_pkt_recvd_time.is_none()
                                {
                                    first_pkt_recvd_time = Some(event_time);
                                }

                                amt_tunnel_updates.push(AmtTunnelUpdate {
                                    time: event_time,
                                    old: amt_tunnel_updated.old,
                                    new: amt_tunnel_updated.new,
                                    trigger: amt_tunnel_updated.trigger,
                                    relay_ip: amt_tunnel_updated.relay_ip,
                                    source_ip: amt_tunnel_updated.source_ip,
                                    group_ip: amt_tunnel_updated.group_ip,
                                });
                            }
                            qlog::events::EventData::FlexicastClientStateUpdated(
                                client_state_updated,
                            ) => {
                                client_state_updates.push(ClientStateUpdate {
                                    time: event_system_time(client_ref_time, qlog_event.time),
                                    channel_id: client_state_updated.channel_id,
                                    client_id: client_state_updated.client_id,
                                    old: client_state_updated.old,
                                    new: client_state_updated.new,
                                });
                            }
                            qlog::events::EventData::FlexicastFallbackUpdated(
                                _fallback_updated,
                            ) => {
                                error!(
                                    "Cannot have FlexicastFallbackUpdated events in client qlogs!"
                                );
                            }
                            _ => (),
                        };
                    }
                    qlog::reader::Event::Json(json_event) => {
                        debug!("json_event={json_event:?}");
                    }
                }
            }
        },
    );

    if let Some(client_ip) = server_client_ip {
        client_src_ip.get_or_insert(client_ip);
    }

    // TODO: pull into another method
    if let Some(fc_reader) = fc_flow_qlog_reader {
        let fc_flow_ref_time = f64_epoch_to_system_time(
            fc_reader
                .qlog
                .trace
                .common_fields
                .as_ref()
                .map(|com| com.reference_time.unwrap())
                .expect("Failed to obtain fc_flow_ref_time"),
        );
        debug!("FC Flow ref time: {fc_flow_ref_time:?}");

        fc_reader.into_iter().for_each(|event| match event {
            qlog::reader::Event::Qlog(qlog_event) => {
                trace!("server fc flow event: {qlog_event:?}");
                match qlog_event.data {
                    qlog::events::EventData::MetricsUpdated(metrics_updated) => {
                        if let Some(cwnd) = metrics_updated.congestion_window {
                            fc_flow_cwnd.push(FcFlowCwnd {
                                time: event_system_time(fc_flow_ref_time, qlog_event.time),
                                congestion_window: cwnd,
                            });
                        }

                        // some metrics_updated events may only contain the bytes in flight,... but if latest_rtt is present, then smoothed_rtt will be as well
                        // (because the smoothed rtt starts at 333ms since it's the default value at the start)
                        if let Some(latest_ms) = metrics_updated.latest_rtt {
                            fc_flow_rtt_samples.push(RttSample {
                                time: event_system_time(fc_flow_ref_time, qlog_event.time),
                                latest_ms,
                                smoothed_ms: metrics_updated.smoothed_rtt,
                            });
                        }
                    }
                    qlog::events::EventData::PacketSent(packet_sent) => {
                        if let Some(pn) = packet_sent.header.packet_number
                            && let Some(&client_received_time) = mc_recv_by_pn.get(&pn)
                        {
                            let sent_time = event_system_time(fc_flow_ref_time, qlog_event.time);
                            mc_one_way_delays.push(FcFlowOneWayDelay {
                                time: sent_time,
                                delay: client_received_time.duration_since(sent_time).ok(),
                            }); // can be None if the client clock is behind
                        }
                    }
                    _ => (),
                };
            }
            qlog::reader::Event::Json(json_event) => {
                debug!("json_event={json_event:?}");
            }
        });
    }

    let time_to_first_data = between(open_time, first_stream_data_time);

    let startup_phases = StartupPhases {
        start_to_mc_join: between(open_time, mc_join_time),
        join_to_mc_timeout: between(mc_join_time, amt_opening_time),
        amt_opening_to_opened: between(amt_opening_time, amt_opened_time),
        amt_opened_to_first_amt_pkt: between(amt_opened_time, first_pkt_recvd_time),
        amt_first_packet_to_first_stream_frm: between(first_pkt_recvd_time, first_stream_data_time),
    };

    // none if the qlog didn't contain from_mc or if no mcast packets were seen
    let mcast_loss_rate = (total_mcast_packet > 0)
        .then(|| mcast_losses as f64 / (total_mcast_packet + mcast_losses) as f64);

    // a dummy client is used to establish the mc flow, it acks the first packet, so we need to drop it in order to compute the mc losses based on the first actually received mc packet for the client
    server_acked_mc_pns.pop_first();

    // compute the lost packets based on the PATH_ACKs received from the client
    // the packets that are missing between the first and the last acked mc flow packets were lost. Could be None only if the client never acked anything from the fc flow
    // NOTE: here we do still count the packets that were sent over the fc flow while the clietn was in unicast fallback as lost, but we can rely on the client side metrics in that case (if they diverge too much)
    let server_pov_mcast_span = server_acked_mc_pns
        .first() // take the first ACKed pn on the fc flow
        .zip(server_acked_mc_pns.last()) // take the last ACKed pn on the fc flow
        .map(|(first, last)| last - first + 1); // if both are Some, then compute the span of ACKed packets on the fc flow

    // based on the acked span, compute the number of missed packets
    let server_pov_mcast_losses =
        server_pov_mcast_span.map(|span| span - server_acked_mc_pns.len() as u64);

    let server_pov_mcast_loss_rate = server_pov_mcast_span
        .zip(server_pov_mcast_losses)
        .map(|(span, losses)| losses as f64 / span as f64);

    info!(
        "mcast losses: client pov {mcast_losses}/{} (rate {mcast_loss_rate:?}), server pov {server_pov_mcast_losses:?}/{server_pov_mcast_span:?} (rate {server_pov_mcast_loss_rate:?})",
        total_mcast_packet + mcast_losses
    );

    info!(
        "{} retransmitted stream frames, {} of them duplicates",
        retransmission_events.len(),
        retransmission_events.iter().filter(|r| r.duplicate).count()
    );

    // find large multicast packet gaps as being 1.5 times the FC_PING interval, i.e., at least once lost FC_PING frame
    let large_mc_packet_gap: Vec<Duration> = mc_packet_received_times
        .array_windows::<2>()
        .filter_map(|&[a, b]| b.duration_since(a).ok())
        .filter(|dur| dur.as_millis() > (announced_fc_ping_interval_ms * 3 / 2) as u128)
        .collect();

    debug!("large_mc_packet_gap: {:?}", large_mc_packet_gap);

    let payload_app_latency: Vec<Duration> = application_data_latency
        .iter()
        .filter_map(|(size, lat)| if *size > 1350u64 { Some(*lat) } else { None })
        .collect();
    debug!("payload_app_lat: {:?}", payload_app_latency);

    let no_payload_app_latency: Vec<Duration> = application_data_latency
        .iter()
        .filter_map(|(size, lat)| if *size <= 1350u64 { Some(*lat) } else { None })
        .collect();
    debug!("no_payload_app_lat: {:?}", no_payload_app_latency);

    ConnMetrics {
        client_ip: client_src_ip,
        is_persistent_client,
        conn_open_time: open_time,
        conn_close_reason: closed_reason,
        conn_close_time: closed_time,
        rtt_samples: rtt,
        reception_mode_updates,
        amt_tunnel_updates,
        client_state_updates,
        server_pov_client_state_updates,
        fallback_updates,
        retransmission_events,
        multicast_losses: mcast_losses,
        total_multicast_packets: total_mcast_packet,
        mcast_loss_rate,
        server_pov_multicast_losses: server_pov_mcast_losses,
        server_pov_mcast_loss_rate,
        client_received_bytes: received_bytes,
        client_sent_bytes: sent_bytes,
        time_to_first_data,
        startup_phases,
        large_mc_packet_gaps: large_mc_packet_gap,
        payload_app_latency,
        no_payload_app_latency,
        server_uc_retransmissions,
        fc_flow_cwnd,
        fc_flow_rtt_samples,
        fc_flow_one_way_latency: mc_one_way_delays,
    }
}

fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let args = Args::parse();

    let out_dir = Path::new(&args.out_path);
    match std::fs::create_dir_all(out_dir) {
        Ok(_) => (),
        Err(e) => info!("Failed to create output dir: {e:?}"),
    }

    let qlog_dir = Path::new(&args.in_path);
    let (logs_map, fc_flow_qlogs_map) = read_qlog_dir(qlog_dir);

    debug!("logs_map: {:?}", logs_map);

    // one entry per client connection that has both its client and server qlogs

    let all_metrics: Vec<ConnMetrics> = logs_map.into_par_iter().filter_map(|(key, val)| {
        let (client_filename, server_filename) =
            (val.client_log_filename?, val.server_log_filename?);

            info!(
                "Client connection with dcid={} has the following qlog files: client={:?} and server={:?}",
                key, client_filename, server_filename
            );

            let client_file = File::open(client_filename).expect("Failed to open client qlog file");
            let server_file = File::open(server_filename).expect("Failed to open server qlog file");

            let client_conn_metrics =
                parse_client_conn(client_file, server_file, &fc_flow_qlogs_map);
            // info!("Client connection metrics: {:?}", client_conn_metrics);

            let filename = format!("{}/connection_{}_{}.json", out_dir.to_string_lossy(), client_conn_metrics.client_ip.map(|ip| ip.to_string()).unwrap_or("0.0.0.0".to_string()), key);
            let mut out_file = BufWriter::new(File::create(filename).unwrap());

            serde_json::to_writer(&mut out_file, &client_conn_metrics).unwrap();
            out_file.flush().unwrap();

            Some(client_conn_metrics)

    }).collect();

    info!("parsed {} client connections", all_metrics.len());
}
