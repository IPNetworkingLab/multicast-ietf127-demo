#!/usr/bin/env bash

MQ=/home/corentin/multicast-quic
ID=${1:-1}
OUT_DIR=./fcquic-client$ID

mkdir -p "$OUT_DIR"
# remove the previous run's files
rm -f "$OUT_DIR"/heartbeat_*

# the fcquic receiver will receive files and will store them in $OUT_DIR/heartbeat_n with n being the current heartbeat counter for this receiver,
# not the global heartbeat counter, i.e., it always starts at 0.
# the bash script reads the output of the receiver and when it reads "EVENT finished-client", it will print the first line of the heartbeat file
ip netns exec client$ID env RUST_LOG=debug "$MQ/target/release/fc-recv-file-transfer" https://10.10.0.2:4433/heartbeat \
    -l 10.10.$ID.2 --flexicast --stay-open --transfer-kind "file,file,$OUT_DIR" 2> "$OUT_DIR/receiver.log" \
    | {
        n=0
        while read -r line; do
            if [ "$line" = "EVENT finished-client" ]; then
                head -n 1 "$OUT_DIR/heartbeat_$n"
                n=$((n + 1))
            fi
        done
    }
