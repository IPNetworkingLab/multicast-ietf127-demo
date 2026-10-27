docker run --rm -it --name fcquic-recv1 \
  --network host \
  -v "$PWD/out/client1:/bin-fcquic-client1" \
  fcquic-beacon-receiver 193.191.171.131 1
