#!/usr/bin/env bash

# usage: ./run_tfc_receiver.sh [client ID] [extra args passed to the app binary...]
# set AMT_RELAY=<ip> to allow the AMT fallback through that relay.

BEACON_DIR="$(cd "$(dirname "$0")" && pwd)"
TFC_RECEIVER="$BEACON_DIR/target/release/tfc-receiver"

ID=${1:-1}
shift # shift the positional arguments of the script by 1 to the left, so 2 becomes 1,..
# this is to allow us to pass the ID to this script, but also have extra args that can be passed to the binary

NS=${NS:-client$ID}
SRC_IP=${SRC_IP:-10.10.0.2}
LOCAL_IP=${LOCAL_IP:-10.10.$ID.2}

AMT_ARGS=()
if [ -n "$AMT_RELAY" ]; then
    AMT_ARGS=(--amt-fallback --amt-relay "$AMT_RELAY")
fi

ip netns exec "$NS" env RUST_LOG=debug RUST_BACKTRACE=full "$TFC_RECEIVER" \
    "https://$SRC_IP:4433/heartbeat" -l "$LOCAL_IP" --flexicast "${AMT_ARGS[@]}" "$@"
