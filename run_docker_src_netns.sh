docker run --rm -it --name fcquic-source --privileged  -v /run/netns:/run/netns:rslave -e NS=server1 -v "$PWD/out/source:/source-heartbeats" fcquic-beacon-source "$@"
