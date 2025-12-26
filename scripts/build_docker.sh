#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME="${1:-sfwwslm/axo-drive}"
IMAGE_TAG="${2:-}"
DOCKERFILE_PATH="${DOCKERFILE_PATH:-Dockerfile}"

if [[ ! -f "$DOCKERFILE_PATH" ]]; then
  echo "Dockerfile not found at ${DOCKERFILE_PATH}" >&2
  exit 1
fi

if [[ -z "$IMAGE_TAG" ]]; then
  IMAGE_TAG="$(awk -F '\"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
  IMAGE_TAG="${IMAGE_TAG:-latest}"
fi

docker build -f "$DOCKERFILE_PATH" -t "${IMAGE_NAME}:${IMAGE_TAG}" .
