#!/bin/bash

# receiver for the FC_PING beacon (run_docker_fcquic_beacon_fcping.sh), runs alongside run_docker_recv.sh

# extra args are passed to tfc-receiver, e.g. ./run_docker_recv.sh --pretty-print false

NAME=fcquic-recv-fcping
LOG_DIR="$PWD/out/client-fcping"
mkdir -p "$LOG_DIR"

# save the logs of the previous run before removing its container
if docker container inspect "$NAME" >/dev/null 2>&1; then
  PREV_LOG="$LOG_DIR/receiver-$(date +%Y%m%d-%H%M%S).log"
  echo "Saving the previous run's logs to $PREV_LOG"
  docker logs -t "$NAME" > "$PREV_LOG" 2>&1
fi

# remove the container from a previous run
docker rm -f "$NAME" >/dev/null 2>&1

# STOP_ONLY=1 will save the logs and remove the container
[ -n "$STOP_ONLY" ] && exit 0

docker run -d --name "$NAME" --init --network host \
  --log-opt mode=non-blocking --log-opt max-buffer-size=16m \
  --log-opt max-size=200m --log-opt max-file=10 \
  ${RUST_LOG:+-e RUST_LOG="$RUST_LOG"} \
  -e SRC_PORT=4435 \
  -v "$LOG_DIR:/bin-fcquic-client1" \
  corentindetry/fcquic-beacon-receiver:fcping 193.191.171.131 1 "$@" || exit 1

docker logs -f "$NAME"
