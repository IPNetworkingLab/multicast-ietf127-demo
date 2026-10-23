#!/usr/bin/env bash

MQ=/home/corentin/multicast-quic
BEACON=/home/corentin/multicast-ietf127-demo/fcquic-beacon/target/release/source
SOCK=/tmp/fcquic-source.sock

# The beacon sends relative filenames, so both processes run from this directory.
WORK_DIR=./source-heartbeats

mkdir -p "$WORK_DIR"
cd "$WORK_DIR"
rm -f "$SOCK"

# the source binds to a socket with the address $SOCK
ip netns exec server1 env RUST_LOG=debug "$MQ/target/release/fc-flow-file-transfer" \
    --flexicast --unicast --src 10.10.0.2:4433 --mc-src-addr 10.10.0.2:4443 --fc-timer 0 \
    --cert-path "$MQ/apps/src/bin" --transfer-kind "file,socket,$SOCK" > source.log 2>&1 &
SRC_PID=$!

# kill the source when this script exits
trap 'kill $SRC_PID 2>/dev/null' EXIT

# wait for the source to bind its socket before starting the beacon.
while [ ! -S "$SOCK" ]; do
    kill -0 $SRC_PID 2>/dev/null || { echo "source exited, see $WORK_DIR/source.log"; exit 1; }
    sleep 0.1
done

# the source binary will send filenames to the source via a socket using the source address $SOCK
RUST_LOG=debug "$BEACON" "$SOCK" "$WORK_DIR" "$@"
