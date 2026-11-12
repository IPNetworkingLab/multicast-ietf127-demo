#!/bin/bash

# usage: entrypoint.sh SRC_IP [ID] [extra args are all passed to the binary...]
# env vars: NS (optional network namespace), AMT_RELAY (defaults to 62.40.96.44 if not set), SRC_PORT (default 4433),
# QLOGDIR (default /bin-fcquic-client$ID/qlogs), QLOG_UPLOAD_URL (default http://SRC_IP:8080/qlog)

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

QLOGDIR=${QLOGDIR-/bin-fcquic-client$ID/qlogs}
QLOG_UPLOAD_URL=${QLOG_UPLOAD_URL:-http://$SRC_IP:8080/qlog}
QLOG_ENV=()
if [ -n "$QLOGDIR" ]; then
    mkdir -p "$QLOGDIR"
    QLOG_ENV=(QLOGDIR="$QLOGDIR")
fi

NETNS=()
if [ -n "$NS" ]; then
    NETNS=(ip netns exec "$NS")
fi

echo "Starting receiver $ID towards $SRC_IP:$SRC_PORT"

trap 'kill -INT $PID 2>/dev/null' INT TERM
setsid "${NETNS[@]}" env --default-signal=INT "${QLOG_ENV[@]}" RUST_LOG="${RUST_LOG:-warn,tfc_receiver=info}" RUST_BACKTRACE=full "$TFC_RECEIVER" \
    "https://$SRC_IP:$SRC_PORT/heartbeat" -l 0.0.0.0 --flexicast \
    --amt-fallback --amt-relay "$AMT_RELAY" "$@" &
PID=$!
while kill -0 $PID 2>/dev/null; do wait $PID; done

if [ -n "$QLOGDIR" ]; then
    for f in "$QLOGDIR"/*.sqlog; do [ -f "$f" ] && gzip -f "$f"; done
    for f in "$QLOGDIR"/*.sqlog.gz; do
        # -f: fail on HTTP errors so the file isn't removed, -sS: no progress bar but still print errors
        # -m 60: 60s timeout, -T: PUT the file, the trailing / makes curl append the filename to the URL
        [ -f "$f" ] && curl -fsS -m 60 -T "$f" "$QLOG_UPLOAD_URL/" && rm "$f" && echo "Uploaded $f"
    done
fi
