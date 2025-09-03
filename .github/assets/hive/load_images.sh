#!/usr/bin/env bash
# Adapted from reth's load_images.sh
# Original: https://github.com/paradigmxyz/reth/blob/main/.github/assets/hive/load_images.sh
set -eo pipefail

# List of tar files to load
IMAGES=(
    "/tmp/hiveproxy.tar"
    "/tmp/devp2p.tar"
    "/tmp/engine.tar"
    "/tmp/rpc_compat.tar"
    "/tmp/smoke_genesis.tar"
    "/tmp/smoke_network.tar"
    "/tmp/ethereum_sync.tar"
    "/tmp/eest_engine.tar"
    "/tmp/eest_rlp.tar"
    "/tmp/bera-reth_image.tar"
    "/tmp/berachain_rpc_compat.tar"
)

# Loop through the images and load them
for IMAGE_TAR in "${IMAGES[@]}"; do
    echo "Loading image $IMAGE_TAR..."
    docker load -i "$IMAGE_TAR" &
done

wait

docker image ls -a