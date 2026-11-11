#!/bin/bash

# usage: entrypoint.sh SRC_IP [ID] [extra args are all passed to the binary...]
# env vars: NS (optional network namespace), AMT_RELAY (defaults to 62.40.96.44 if not set), SRC_PORT (default 4433)

TFC_RECEIVER=/tfc-receiver

SRC_IP=$1
ID=${2:-1}

# we want the remaining arguments to be given to the binary
# so shift the remaining arguments 2 if 2 or more args are given (i.e. if both the src ip and id
# were give, then shift left by 2 to allow anything that comes after to be handed with $@ to the binary)
# if less than 2 were given, then shift by however many were given
shift $(( $# < 2 ? $# : 2 ))

if [ -z "$SRC_IP" ]; then
    echo "usage: $0 SRC_IP [ID] [extra tfc-receiver args...]"
    exit 1
fi

AMT_RELAY=${AMT_RELAY:-62.40.96.44}
SRC_PORT=${SRC_PORT:-4433}

NETNS=()
if [ -n "$NS" ]; then
    NETNS=(ip netns exec "$NS")
fi

echo "Starting receiver $ID towards $SRC_IP:$SRC_PORT"

# exec is used to pass signals directly (e.g., ctrl c)
exec "${NETNS[@]}" env RUST_LOG="${RUST_LOG:-warn,tfc_receiver=info}" RUST_BACKTRACE=full "$TFC_RECEIVER" \
    "https://$SRC_IP:$SRC_PORT/heartbeat" -l 0.0.0.0 --flexicast \
    --amt-fallback --amt-relay "$AMT_RELAY" "$@"
