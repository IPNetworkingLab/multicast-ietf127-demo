#!/usr/bin/env bash

# runs the FC_PING source container (fcquic-beacon-source:fcping) in a network namespace, like run_tfc_source.sh
# extra arguments are passed to the app, e.g. ./run_docker_tfc_source_fcping.sh --send-interval 1000

BEACON_DIR="$(cd "$(dirname "$0")" && pwd)"

NS=${NS:-server1}
SRC_IP=${SRC_IP:-10.10.0.2}

# created here so that it isn't created by docker as root
# the mc flow keylog file (fc-flow-keylog.txt) is written there
LOG_DIR="$BEACON_DIR/tfc-source-fcping"
mkdir -p "$LOG_DIR"

docker run --rm -it --name fcquic-source-fcping-test --privileged \
  -v /run/netns:/run/netns:rslave -e NS="$NS" -e SRC_IP="$SRC_IP" \
  -e SRC_PORT=4435 -e MC_SRC_PORT=4444 \
  -e RUST_LOG="${RUST_LOG:-trace}" \
  -v "$LOG_DIR:/source-heartbeats" \
  fcquic-beacon-source:fcping --mc-addr 239.239.239.36:4434 "$@"
