# multicast-ietf127-demo

## Running the demo

To run a receiver and join the demo, you only need docker installed, then run the demo script:
```
./run_demo.sh
```
This script simply runs the `corentindetry/fcquic-beacon-receiver:fcping` container (whose entrypoint script is [fcquic-beacon-receiver/entrypoint.sh](./containers/fcquic-beacon-receiver/entrypoint.sh)).
Extra arguments are passed to the receiver binary [`tfc-receiver.rs`](fcquic-beacon/src/bin/tfc-receiver.rs), e.g. `./run_demo.sh --pretty-print false`.

The receiver connects to the Flexicast source (193.191.171.131:4435) and receives heartbeats over multicast. 
Each heartbeat is a JSON message containing a session id, sequence number, timestamp, and interval,... Heartbeats are sent every 4 seconds, and every 10th heartbeat also carries a random 10 kilobytes payload, the sha256 hash of the payload is sent in the heartbeat to check integrity.

Use ctrl-c to stop the receiver. When the container is stopping, the QLOGs of the connection are sent over to our source server for analysis.

 **NOTICE:** We will collect the following information about the connection:
 - Non-anonymized IP address
 - Complete session logs 
  - QLOGs include all packets sent and received by the client at the transport level.
 - AMT, Unicast fallback events
 - Retransmissions,...
 
This information will only be used for research purposes, and will allow us to improve our Flexicast QUIC implementation, find new potential issues, and gain insights only attainable from real deployments.
 

### Source
The source server is located in the UCLouvain network, connected via a GRE tunnel to Belnet, the belgian education network operator, which has multicast connectivity with the GEANT network.

### Multicast fallback for receivers in non multicast networks

Multicast connectivity with the GEANT network is possible at universities in Europe, but is unlikely in most case. Thankfully, GEANT has a set of AMT (Automatic Multicast Tunneling [RFC7450](https://datatracker.ietf.org/doc/html/rfc7450)) Relays that enable clients in non-multicast networks to create on-demand unicast tunnels to receive multicast content.

Our demonstration makes use of these relays to provide client initiated multicast fallback to AMT if the client notices it isn't receiving multicast data natively. This is parallel to the Unicast fallback offered by Flexicast QUIC, in which a client that doesn't meet the group's minimum network quality is evicted and receives content through unicast only. This unicast fallback is also applicable for clients in non multicast network, but is not in use in this demonstration in order for the source to distribute all of its content via multicast, and rely on the network to reach clients, wherever they are.

The AMT relays in the GEANT network all advertise the Anycast address `62.40.96.44`.


## FCQUIC-Beacon

### Docker containers

To run a flexicast QUIC receiver, or a source, you can use the provided images:
- `fcquic-beacon-receiver`: Flexicast QUIC heartbeat receiver
  - usage: `SRC_IP [ID] [extra arguments passed to the binary]`
  - Optional env vars:
    - SRC_PORT: UC source port (default 4433, 4435 for the FC_PING source)
    - AMT_RELAY: AMT relay used for fallback when multicast doesn't work for the receiver (default 62.40.96.44)
    - QLOGDIR: directory where the qlogs are written (default /bin-fcquic-client$ID/qlogs). You can disable qlog collection by setting it to an empty string
    - QLOG_UPLOAD_URL: where the qlogs are uploaded when the receiver stops (default http://SRC_IP:8080/qlog)
  - Tags: `ping` for the PING source, `fcping` for the FC_PING source
    - There are two FCQUIC sources running, one with QUIC's PING frames sent on the multicast flow every 250ms, however this frame is ack-eliciting, which induces load on the source at high receiver counts. The FC_PING frame was therefore introduced, which is by default non ack-eliciting, but may carry information to identify a group of receivers that the sources wants to obtain ACKs for.

- `fcquic-beacon-source`: Flexicast QUIC heartbeat source
  - Several env vars must be set to run the source:
    - SRC_IP: unicast source address
    - MC_SRC_IP: Source address of the multicast content, may be different when the source sends its multicast content through a tunnel to a multicast enabled network (default SRC_IP)
    - SRC_PORT: UC source port (default 4433)
    - MC_SRC_PORT: Multicast content source port (default 4443)
    - FC_PING_INTERVAL: Interval of the FC_PING frames to be sent on the multicast flow when idle. A lower value helps reduce the time it takes for a receiver to fallback on AMT. 
      With the default settings, the source will requests ACKs to the first 30 FC_PING frames for each new client. This is done in groups of 250 receivers.
      (in ms, default 250, receivers fallback on AMT after 3x this interval without multicast packets)
    - QLOGDIR: directory where the qlogs of the FC flow and UC paths are written (default /source-heartbeats/qlogs)
 
- `picoquic`: Picoquic implementation of an FCQUIC receiver, by Ambroise Sirven (resowifix)

Every container is built for arm64 and amd64 and have been tested on Linux and MacOS (27.2.1).

### Run scripts
- `run_demo.sh`: run a basic FCUQIC receiver, see [Running the demo](#running-the-demo)
- `run_docker_fcquic_beacon.sh`: PING source (port 4433)
- `run_docker_fcquic_beacon_fcping.sh`: FC_PING source (port 4435, multicast on 239.239.239.36:4434)
- `run_docker_recv.sh`: receiver for the PING source
- `run_docker_recv_fcping.sh`: receiver for the FC_PING source, to run alongside run_docker_recv.sh
- `run_qlog_server.sh`: qlog upload server, runs on the source VM (port 8080)
- `run_docker_recv_netns.sh`, `run_docker_src_netns.sh`: receiver and source in local network namespaces, for tests


### QLOGs
Quiche's qlog crate was extended to not only carry the Flexicast QUIC frames, but also include additional events, such as AMT Fallback triggered, AMT Tunnel closed, Multicat flow joined,...

When the FCQUIC receiver container stops, its qlogs are gzipped and uploaded to the source in order to be parsed and analyzed later.

### Binaries
The two following binaries are implemented as standalone applications that do not depend on the FCQUIC implementation. Rather, the source writes files to disk, then sends the filename over a UnixDatagram socket to the FCQUIC source binary, which then transmits the file using multicast to the FCQUIC receiver, which will write it to disk, and send the filename to the receiver application over a UnixDatagram socket.
- `source.rs` 
- `receiver.rs`

The following binaries are applications built using `tokio-fcquiche`, thereby depending on the full FCQUIC applications. In the source binary, the beacon creation and transmission is done in a tokio task, and transmission over multicast,... is handled by `tokio-fcquiche`, which exposes a channel to send information over multicast (and/or unicast), and another channel to receive information from the clients,...
The receiver binary is identical in its architecture.
- `tfc-source.rs`
- `tfc-receiver.rs`
