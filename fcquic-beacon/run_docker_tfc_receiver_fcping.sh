#!/usr/bin/env bash

# runs the FC_PING receiver container (fcquic-beacon-receiver:fcping) in a network namespace, like run_tfc_receiver.sh
# usage: ./run_docker_tfc_receiver_fcping.sh [client ID] [extra args passed to the app binary...]

ID=${1:-1}
shift # the remaining args are passed to the binary

NS=${NS:-client$ID}
SRC_IP=${SRC_IP:-10.10.0.2}

docker run --rm -it --name "fcquic-recv$ID-fcping-test" --privileged \
  -v /run/netns:/run/netns:rslave -e NS="$NS" -e SRC_PORT=4435 \
  -e RUST_LOG="${RUST_LOG:-trace}" \
  fcquic-beacon-receiver:fcping "$SRC_IP" "$ID" "$@"
