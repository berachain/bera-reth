# syntax=docker.io/docker/dockerfile:1.7-labs
# Adapted from reth's hive Dockerfile
# Original: https://github.com/paradigmxyz/reth/blob/main/.github/assets/hive/Dockerfile

# 
# We'll use cargo-chef to speed up the build
# 
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config

# 
# We prepare the build plan
# 
FROM chef AS planner

ARG CARGO_BIN

COPY --exclude=.git --exclude=dist . .
RUN cargo chef prepare --recipe-path recipe.json --bin ${CARGO_BIN}

# 
# And build the app
# 
FROM chef AS builder
WORKDIR /app

ARG CARGO_BIN
ARG BUILD_PROFILE=hivetests
ARG FEATURES=""

COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook \
    --profile $BUILD_PROFILE \
    --bin $CARGO_BIN \
    ${FEATURES:+--features "$FEATURES"}  \
    --recipe-path recipe.json

COPY --exclude=.git --exclude=dist . .
RUN cargo build \
    --profile $BUILD_PROFILE \
    --bin $CARGO_BIN \
    ${FEATURES:+--features "$FEATURES"}  \
    --locked

# 
# The runtime will then just use the build artifact without building anything
# 
FROM ubuntu AS runtime

ARG CARGO_BIN

COPY --from=builder /app/target/hivetests/$CARGO_BIN /usr/local/bin/bera-reth
COPY LICENSE-* ./

EXPOSE 30303 30303/udp 9001 8545 8546
ENV RUST_LOG=debug
ENTRYPOINT ["/usr/local/bin/bera-reth"]