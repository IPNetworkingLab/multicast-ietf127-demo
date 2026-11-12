#!/bin/bash

docker run --rm -it --init --network host --pull always \
  -e RUST_LOG=info -e SRC_PORT=4435 \
  corentindetry/fcquic-beacon-receiver:fcping 193.191.171.131 1 "$@"
