#!/bin/bash

# env vars: NS (optional network namespace), SRC_IP, MC_SRC_IP (defaults to SRC_IP),
# SRC_PORT (default 4433), MC_SRC_PORT (default 4443), FC_PING_INTERVAL (in ms, default 250),
# QLOGDIR (default /source-heartbeats/qlogs)

TFC_SOURCE=/tfc-source

# the mc flow keylog file is written in the current dir
WORK_DIR=/source-heartbeats
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

NETNS=()
if [ -n "$NS" ]; then
    NETNS=(ip netns exec "$NS")
    # source ip in local network namespace tests
    SRC_IP=${SRC_IP:-10.10.0.2}
fi
MC_SRC_IP=${MC_SRC_IP:-$SRC_IP}
SRC_PORT=${SRC_PORT:-4433}
MC_SRC_PORT=${MC_SRC_PORT:-4443}
# receivers fall back to AMT after 3x this interval without multicast packets
FC_PING_INTERVAL=${FC_PING_INTERVAL:-250}

QLOGDIR=${QLOGDIR-$WORK_DIR/qlogs}
QLOG_ENV=()
if [ -n "$QLOGDIR" ]; then
    mkdir -p "$QLOGDIR"
    QLOG_ENV=(QLOGDIR="$QLOGDIR")
fi

if [ -z "$SRC_IP" ]; then
    echo "SRC_IP must be set when not running in a network namespace"
    exit 1
fi

# exec so that tfc-source receives the signals directly
exec "${NETNS[@]}" env "${QLOG_ENV[@]}" RUST_LOG="${RUST_LOG:-warn,tfc_source=info}" RUST_BACKTRACE=full "$TFC_SOURCE" \
    --flexicast --unicast --src "$SRC_IP:$SRC_PORT" --mc-src-addr "$MC_SRC_IP:$MC_SRC_PORT" --fc-timer 0 \
    --fall-back-delay 1000 --initial-fc-flow 100000000000 --cert-path / --fc-ping-interval "$FC_PING_INTERVAL" \
    "$@"
