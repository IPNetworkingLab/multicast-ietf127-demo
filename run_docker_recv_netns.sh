docker run --rm -it --name fcquic-recv1 \
  --privileged \
  -v /run/netns:/run/netns:rslave \
  -v "$PWD/out/client1:/bin-fcquic-client1" \
  -e NS=client1 \
  corentindetry/fcquic-beacon-receiver 10.10.0.2 1
