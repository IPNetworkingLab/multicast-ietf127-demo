#!/bin/bash


BEACON=/source
SOCK=/tmp/fcquic-source.sock

# The beacon sends relative filenames, so both processes run from this directory.
WORK_DIR=/source-heartbeats

mkdir -p "$WORK_DIR"
cp /cert.key $WORK_DIR
cp /cert.crt $WORK_DIR

cd "$WORK_DIR"
rm -f "$SOCK"


# the source binds to a socket with the address $SOCK
if [ -z "$NS" ]; then
    # SRC_IP=193.190.248.24
    env RUST_LOG=debug RUST_BACKTRACE=full "/fc-flow-file-transfer" \
        --flexicast --unicast --src ${SRC_IP}:4433 --mc-src-addr ${MC_SRC_IP}:4443 --fc-timer 0 \
        --fall-back-delay 1000 --initial-fc-flow 100000000000 --cert-path "." --transfer-kind "file,socket,$SOCK" --fc-keepalive 250 > source.log 2>&1 &
else
    SRC_IP=10.10.0.2
    ip netns exec $NS env RUST_LOG=debug RUST_BACKTRACE=full "/fc-flow-file-transfer" \
        --flexicast --unicast --src ${SRC_IP}:4433 --mc-src-addr ${SRC_IP}:4443 --fc-timer 0 \
        --fall-back-delay 1000 --initial-fc-flow 100000000000 --cert-path "." --transfer-kind "file,socket,$SOCK" --fc-keepalive 250 > source.log 2>&1 &
fi
SRC_PID=$!

# kill the source when this script exits
trap 'kill $SRC_PID 2>/dev/null' EXIT

# wait for the source to bind its socket before starting the beacon.
while [ ! -S "$SOCK" ]; do
    kill -0 $SRC_PID 2>/dev/null || { echo "source exited, see $WORK_DIR/source.log"; exit 1; }
    sleep 0.1
done

# the source binary will send filenames to the source via a socket using the source address $SOCK
# we already cd'd into $WORK_DIR, so the heartbeats are written to the current directory
RUST_LOG=debug "$BEACON" "$SOCK" . "$@"
