#!/bin/bash

USERNAME="corentindetry"
# multi-arch tag to push, e.g., TAG=fcping ./upload_images.sh
TAG=${TAG:-latest}

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

  # combine the arch images into a single multi arch tag
  echo "creating multi-arch manifest $USERNAME/$IMAGE:$TAG"
  docker buildx imagetools create -t "$USERNAME/$IMAGE:$TAG" "${SOURCES[@]}"
done

echo "done"
