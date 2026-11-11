#!/bin/bash

# env vars: NS (optional network namespace), SRC_IP, MC_SRC_IP (defaults to SRC_IP)
# NOTE: SRC_IP refers to the address that will be used by the source, e.g, this is the one clients connect to
# MC_SRC_IP refers to the multicast source ip, this is different in the case where the multicast traffic exits
# out of a GRE tunnel into a PIM-SSM network, otherwise it can be the same as SRC_IP
#
# the extra args are all passed to the application

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

if [ -z "$SRC_IP" ]; then
    echo "SRC_IP must be set when not running in a network namespace"
    exit 1
fi

# exec so that tfc-source receives the signals directly
exec "${NETNS[@]}" env RUST_LOG="${RUST_LOG:-warn,tfc_source=info}" RUST_BACKTRACE=full "$TFC_SOURCE" \
    --flexicast --unicast --src "$SRC_IP:4433" --mc-src-addr "$MC_SRC_IP:4443" --fc-timer 0 \
    --fall-back-delay 1000 --initial-fc-flow 100000000000 --cert-path / --fc-keepalive 250 \
    "$@"
