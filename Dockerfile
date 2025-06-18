# syntax=docker.io/docker/dockerfile:1.7-labs

# Support setting various labels on the final image
ARG COMMIT=""
ARG VERSION=""
ARG BUILDNUM=""

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

LABEL org.opencontainers.image.source=https://github.com/berachain/bera-reth
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Install system dependencies
RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config

# Builds a cargo-chef plan
FROM chef AS planner
COPY --exclude=.git --exclude=target . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Build profile, release by default
ARG BUILD_PROFILE=release
ENV BUILD_PROFILE=$BUILD_PROFILE

# Extra Cargo flags for optimization and multi-platform support
ARG RUSTFLAGS=""
ARG TARGETPLATFORM
ENV RUSTFLAGS="$RUSTFLAGS"

# Extra Cargo features
ARG FEATURES=""
ENV FEATURES=$FEATURES

# Configure target architecture based on platform
RUN case "$TARGETPLATFORM" in \
    "linux/amd64") echo "x86_64-unknown-linux-gnu" > /tmp/target.txt ;; \
    "linux/arm64") echo "aarch64-unknown-linux-gnu" > /tmp/target.txt ;; \
    "linux/arm/v7") echo "armv7-unknown-linux-gnueabihf" > /tmp/target.txt ;; \
    *) echo "x86_64-unknown-linux-gnu" > /tmp/target.txt ;; \
    esac

# Install target for cross-compilation if needed
RUN TARGET=$(cat /tmp/target.txt) && \
    if [ "$TARGET" != "x86_64-unknown-linux-gnu" ]; then \
        rustup target add $TARGET; \
    fi

# Builds dependencies
RUN TARGET=$(cat /tmp/target.txt) && \
    cargo chef cook --profile $BUILD_PROFILE --features "$FEATURES" --target $TARGET --recipe-path recipe.json

# Build application
COPY --exclude=target . .
RUN TARGET=$(cat /tmp/target.txt) && \
    cargo build --profile $BUILD_PROFILE --features "$FEATURES" --target $TARGET --locked --bin bera-reth

# ARG is not resolved in COPY so we have to hack around it by copying the
# binary to a temporary location
RUN TARGET=$(cat /tmp/target.txt) && \
    cp /app/target/$TARGET/$BUILD_PROFILE/bera-reth /app/bera-reth

# Use Ubuntu as the release image  
FROM ubuntu:24.04 AS runtime
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy bera-reth over from the build stage
COPY --from=builder /app/bera-reth /usr/local/bin/
RUN chmod +x /usr/local/bin/bera-reth

# Copy licenses
COPY LICENSE ./

# Expose standard Ethereum execution client ports
EXPOSE 30303 30303/udp 9001 8545 8546 8551

# Add metadata labels to help programmatic image consumption
ARG COMMIT=""
ARG VERSION=""
ARG BUILDNUM=""
LABEL commit="$COMMIT" version="$VERSION" buildnum="$BUILDNUM"

ENTRYPOINT ["/usr/local/bin/bera-reth"]