#!/bin/bash

USERNAME="corentindetry"

IMAGES=("base" "fcquic-beacon-source" "fcquic-beacon-receiver")
ARCHS=("amd64" "arm64")

for IMAGE in "${IMAGES[@]}"; do
  SOURCES=()
  for ARCH in "${ARCHS[@]}"; do
    echo "tagging and pushing $IMAGE:$ARCH"
    docker tag "$IMAGE:$ARCH" "$USERNAME/$IMAGE:$ARCH"
    docker push "$USERNAME/$IMAGE:$ARCH"
    SOURCES+=("$USERNAME/$IMAGE:$ARCH")
  done

  # combine the arch images into a single multi arch latest tag
  echo "creating multi-arch manifest $USERNAME/$IMAGE:latest"
  docker buildx imagetools create -t "$USERNAME/$IMAGE:latest" "${SOURCES[@]}"
done

echo "done"
