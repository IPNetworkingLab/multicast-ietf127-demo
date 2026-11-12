#!/usr/bin/env bash

# extra arguments are passed to the app, e.g. ./run_tfc_source.sh --send-interval 1000

BEACON_DIR="$(cd "$(dirname "$0")" && pwd)"
TFC_SOURCE="$BEACON_DIR/target/release/tfc-source"

NS=${NS:-server1}
SRC_IP=${SRC_IP:-10.10.0.2}

# mc flow keylog file (fc-flow-keylog.txt) and unicast keylog (uc-keylog.txt)
# are written in the current dir
WORK_DIR=./tfc-source
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

mkdir -p ./qlogs

ip netns exec "$NS" env RUST_LOG=trace RUST_BACKTRACE=full SSLKEYLOGFILE="${SSLKEYLOGFILE:-$PWD/uc-keylog.txt}" QLOGDIR=$PWD/qlogs "$TFC_SOURCE" \
    --flexicast --unicast --src "$SRC_IP:4433" --mc-src-addr "$SRC_IP:4443" --fc-timer 0 \
    --fall-back-delay 1000 --initial-fc-flow 100000000000 --cert-path "$BEACON_DIR" --fc-ping-interval 750 \
    "$@"
