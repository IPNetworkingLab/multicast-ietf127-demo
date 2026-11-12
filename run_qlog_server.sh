#!/bin/bash

# docker build -t qlog-server qlog-server && docker save qlog-server | ssh vm-geant docker load

docker rm -f qlog-server >/dev/null 2>&1
docker run -d --name qlog-server --restart unless-stopped --network host \
  -v "$PWD/qlogs:/data" qlog-server
