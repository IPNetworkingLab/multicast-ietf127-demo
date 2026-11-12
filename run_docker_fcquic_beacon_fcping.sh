#!/bin/bash

NAME=fcquic-source-fcping
LOG_DIR="$PWD/out/source-fcping"
mkdir -p "$LOG_DIR"

# save the logs of the previous run before removing its container
if docker container inspect "$NAME" >/dev/null 2>&1; then
  PREV_LOG="$LOG_DIR/source-$(date +%Y%m%d-%H%M%S).log"
  echo "Saving the previous run's logs to $PREV_LOG"
  docker logs -t "$NAME" > "$PREV_LOG" 2>&1
fi

# remove the container from a previous run
docker rm -f "$NAME" >/dev/null 2>&1

docker run -d --name "$NAME" --init --network host \
  --log-opt mode=non-blocking --log-opt max-buffer-size=16m \
  --log-opt max-size=200m --log-opt max-file=10 \
  -e RUST_LOG="${RUST_LOG:-debug}" \
  -e SRC_IP=193.191.171.131 -e MC_SRC_IP=193.190.248.24 \
  -e SRC_PORT=4435 -e MC_SRC_PORT=4444 \
  -v "$LOG_DIR:/source-heartbeats" \
  corentindetry/fcquic-beacon-source:fcping --mc-addr 239.239.239.36:4434 "$@" || exit 1

docker logs -f "$NAME"
