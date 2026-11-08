#!/bin/bash


# # If NS is not set, then just run the binary as is
# # otherwise run in the specified namespace
# if [ -z "$NS" ]; then
#     # No namespace specified
#     /receiver $@
# else
#     # Run in specific namespace
#     ip netns exec $NS /client $@
# fi


RECEIVER_CLIENT=/receiver
SOCK=/tmp/fcquic-recv.sock
SRC_IP=$1
ID=${2:-1}
OUT_DIR=./bin-fcquic-client$ID

mkdir -p "$OUT_DIR"

# remove the previous run's files
rm -f "$OUT_DIR"/file*

# the fcquic source will create an unbind socket and will send filename information to the beacon receiver binary by using its socket path (in $SOCK)
# so we first delete any previous sock file
rm -f "$SOCK"

if [ -z "$NS" ]; then
    "/picoquic_sf_fc" ${SRC_IP} 4433 62.40.96.44 2268 ${OUT_DIR} 0 > "$OUT_DIR/receiver.log" 2>&1 &
else
    ip netns exec $NS env "/picoquic_sf_fc" ${SRC_IP} 4433 62.40.96.44 2268 ${OUT_DIR} 0 > "$OUT_DIR/receiver.log" 2>&1 &
fi
RECVR_PID=$!

# kill the receiver when this script exits
trap 'kill $RECVR_PID 2>/dev/null' EXIT


RUST_LOG=debug "$RECEIVER_CLIENT" --socket-bind-path "$SOCK"
