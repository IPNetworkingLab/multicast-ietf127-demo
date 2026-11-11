#!/bin/bash

# stop at the first failed build, otherwise the next images are built on top of a stale base
set -e

SKIP_BUILDING_BASE=false
ARCHS=("amd64" "arm64")
while [[ $# -gt 0 ]]; do
  case $1 in
    --skip-base) SKIP_BUILDING_BASE=true; shift ;;
    # comma sep. list e.g. --arch amd64,arm64
    --arch) IFS=',' read -ra ARCHS <<< "$2"; shift 2 ;;
    *) echo "Unknown: $1"; exit 1 ;;
  esac
done

cd /home/corentin/multicast-ietf127-demo

NATIVE_ARCH=$(docker version -f '{{.Server.Arch}}')

# building for a foreign arch needs QEMU binfmt handlers registered in the kernel
# NOTE: this code was done by an LLM 
for ARCH in "${ARCHS[@]}"; do
  if [ "$ARCH" != "$NATIVE_ARCH" ] && ! docker buildx inspect --bootstrap | grep -q "linux/$ARCH"; then
    echo "Installing QEMU emulator for $ARCH"
    docker run --privileged --rm tonistiigi/binfmt --install "$ARCH"
  fi
done

for ARCH in "${ARCHS[@]}"; do
  echo "-------- Building images for linux/$ARCH --------"

  if [ "$SKIP_BUILDING_BASE" = false ]; then
    echo "Building base image"
    docker build --platform "linux/$ARCH" -t "base:$ARCH" \
      --build-context multicast-quic=/home/corentin/multicast-quic \
      -f ./containers/base/Dockerfile .
  else
    echo "Skipping base image build"
  fi

  echo "Building fcquic_client image"
  docker build --platform "linux/$ARCH" -t "fcquic-beacon-source:$ARCH" \
    --build-arg BASE_IMAGE="base:$ARCH" \
    -f ./containers/fcquic-beacon-source/Dockerfile ./containers/fcquic-beacon-source

  echo "Building fcquic_server image"
  docker build --platform "linux/$ARCH" -t "fcquic-beacon-receiver:$ARCH" \
    --build-arg BASE_IMAGE="base:$ARCH" \
    -f ./containers/fcquic-beacon-receiver/Dockerfile ./containers/fcquic-beacon-receiver

  # keep the untagged names pointing to the native arch so local run scripts keep working
  if [ "$ARCH" = "$NATIVE_ARCH" ]; then
    for IMAGE in base fcquic-beacon-source fcquic-beacon-receiver; do
      docker tag "$IMAGE:$ARCH" "$IMAGE:latest"
    done
  fi
done

# TODO: add norm and gpac

echo "Done"
