#!/usr/bin/env bash

MQ=/home/corentin/multicast-quic
RECEIVER_CLIENT=/home/corentin/multicast-ietf127-demo/fcquic-beacon/target/release/receiver
SOCK=/tmp/fcquic-recv.sock
ID=${1:-1}
OUT_DIR=./bin-fcquic-client$ID

mkdir -p "$OUT_DIR"

# remove the previous run's files
rm -f "$OUT_DIR"/heartbeat_*

# the fcquic source will created an unbind socket and will send filename information to the beacon receiver binary by using its socket path (in $SOCK)
# so we first delete any previous sock file
rm -f "$SOCK"

ip netns exec client$ID env RUST_LOG=debug "$MQ/target/release/fc-recv-file-transfer" https://10.10.0.2:4433/heartbeat \
    -l 10.10.$ID.2 --flexicast --stay-open --transfer-kind "file,socket,$SOCK" > "$OUT_DIR/receiver.log" 2>&1 &
RECVR_PID=$!

# kill the receiver when this script exits
trap 'kill $RECVR_PID 2>/dev/null' EXIT


RUST_LOG=debug "$RECEIVER_CLIENT" --socket-bind-path "$SOCK"
