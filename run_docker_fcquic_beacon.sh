docker run --rm -it --name fcquic-source --network host -e SRC_IP=193.191.171.131 -e MC_SRC_IP=193.190.248.24 -v "$PWD/out/source:/source-heartbeats" corentindetry/fcquic-beacon-source:latest "$@"
