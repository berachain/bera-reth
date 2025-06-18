#!/usr/bin/make -f

###############################################################################
###                               Variables                                 ###
###############################################################################

GIT_SHA ?= $(shell git rev-parse HEAD)
GIT_TAG ?= $(shell git describe --tags --abbrev=0 2>/dev/null || echo "dev")
BIN_DIR = "dist/bin"
CARGO_TARGET_DIR ?= target
PROFILE ?= release
# Features: No custom features defined for bera-reth
FEATURES ?=
DOCKER_IMAGE_NAME ?= bera-reth

###############################################################################
###                               Docker                                    ###
###############################################################################

# Note: This requires a buildx builder with emulation support. For example:
#
# `docker run --privileged --rm tonistiigi/binfmt --install amd64,arm64`
# `docker buildx create --use --driver docker-container --name cross-builder`
.PHONY: docker-build-push
docker-build-push: ## Build and push a cross-arch Docker image tagged with the latest git tag.
	$(call docker_build_push,$(GIT_TAG),$(GIT_TAG))

.PHONY: docker-build-push-latest
docker-build-push-latest: ## Build and push a cross-arch Docker image tagged with the latest git tag and `latest`.
	$(call docker_build_push,$(GIT_TAG),latest)

.PHONY: docker-build-push-git-sha
docker-build-push-git-sha: ## Build and push a cross-arch Docker image tagged with the latest git sha.
	$(call docker_build_push,$(GIT_SHA),$(GIT_SHA))

.PHONY: docker-build-local
docker-build-local: ## Build a Docker image for local use.
	docker build --tag $(DOCKER_IMAGE_NAME):local \
		--build-arg COMMIT=$(GIT_SHA) \
		--build-arg VERSION=$(GIT_TAG) \
		--build-arg BUILD_PROFILE=$(PROFILE) \
		.

.PHONY: docker-build-push-nightly
docker-build-push-nightly: ## Build and push cross-arch Docker image tagged with nightly.
	$(call docker_build_push,nightly,nightly)

.PHONY: docker-build-push-nightly-profiling
docker-build-push-nightly-profiling: ## Build and push cross-arch Docker image with profiling profile tagged with nightly-profiling.
	$(call docker_build_push,nightly-profiling,nightly-profiling)

# Create a cross-arch Docker image with the given tags and push it
define docker_build_push
	@echo "=== Building x86_64 binary ==="
	$(MAKE) build-x86_64-unknown-linux-gnu
	@echo "Creating dist directories..."
	mkdir -p $(BIN_DIR)/amd64
	@echo "Listing target directory contents:"
	ls -la $(CARGO_TARGET_DIR)/x86_64-unknown-linux-gnu/$(PROFILE)/ || echo "Target directory does not exist"
	@echo "Copying x86_64 binary..."
	cp $(CARGO_TARGET_DIR)/x86_64-unknown-linux-gnu/$(PROFILE)/bera-reth $(BIN_DIR)/amd64/bera-reth
	@echo "Verifying x86_64 binary copied successfully..."
	test -f $(BIN_DIR)/amd64/bera-reth && ls -la $(BIN_DIR)/amd64/bera-reth

	@echo "=== Building aarch64 binary ==="
	$(MAKE) build-aarch64-unknown-linux-gnu
	mkdir -p $(BIN_DIR)/arm64
	@echo "Listing target directory contents:"
	ls -la $(CARGO_TARGET_DIR)/aarch64-unknown-linux-gnu/$(PROFILE)/ || echo "Target directory does not exist"
	@echo "Copying aarch64 binary..."
	cp $(CARGO_TARGET_DIR)/aarch64-unknown-linux-gnu/$(PROFILE)/bera-reth $(BIN_DIR)/arm64/bera-reth
	@echo "Verifying aarch64 binary copied successfully..."
	test -f $(BIN_DIR)/arm64/bera-reth && ls -la $(BIN_DIR)/arm64/bera-reth

	@echo "=== Final verification ==="
	@echo "Contents of dist/bin:"
	find $(BIN_DIR) -type f -exec ls -la {} \;
	@echo "Building and pushing Docker image..."
	docker buildx build --file ./Dockerfile.cross . \
		--platform linux/amd64,linux/arm64 \
		--tag $(DOCKER_IMAGE_NAME):$(1) \
		--tag $(DOCKER_IMAGE_NAME):$(2) \
		--build-arg COMMIT=$(GIT_SHA) \
		--build-arg VERSION=$(GIT_TAG) \
		--provenance=false \
		--push
endef

# Cross-compilation targets
build-x86_64-unknown-linux-gnu:
	@echo "=== Building x86_64 binary ==="
	@echo "Checking for cross tool..."
	@command -v cross >/dev/null 2>&1 || { echo "ERROR: cross is not installed. Run: cargo install cross --git https://github.com/cross-rs/cross"; exit 1; }
	@echo "cross tool found at: $$(command -v cross)"
	@echo "Starting cross-compilation for x86_64..."
	RUSTFLAGS="-C link-arg=-lgcc -Clink-arg=-static-libgcc" \
		cross build --bin bera-reth --target x86_64-unknown-linux-gnu --features "$(FEATURES)" --profile "$(PROFILE)" --verbose
	@echo "Cross-compilation completed. Verifying binary exists..."
	@echo "Expected binary location: $(CARGO_TARGET_DIR)/x86_64-unknown-linux-gnu/$(PROFILE)/bera-reth"
	test -f $(CARGO_TARGET_DIR)/x86_64-unknown-linux-gnu/$(PROFILE)/bera-reth || { echo "ERROR: x86_64 binary not found!"; exit 1; }
	@echo "x86_64 binary verified successfully"

build-aarch64-unknown-linux-gnu: export JEMALLOC_SYS_WITH_LG_PAGE=16
build-aarch64-unknown-linux-gnu:
	@echo "=== Building aarch64 binary ==="
	@echo "Checking for cross tool..."
	@command -v cross >/dev/null 2>&1 || { echo "ERROR: cross is not installed. Run: cargo install cross --git https://github.com/cross-rs/cross"; exit 1; }
	@echo "cross tool found at: $$(command -v cross)"
	@echo "Starting cross-compilation for aarch64..."
	RUSTFLAGS="-C link-arg=-lgcc -Clink-arg=-static-libgcc" \
		cross build --bin bera-reth --target aarch64-unknown-linux-gnu --features "$(FEATURES)" --profile "$(PROFILE)" --verbose
	@echo "Cross-compilation completed. Verifying binary exists..."
	@echo "Expected binary location: $(CARGO_TARGET_DIR)/aarch64-unknown-linux-gnu/$(PROFILE)/bera-reth"
	test -f $(CARGO_TARGET_DIR)/aarch64-unknown-linux-gnu/$(PROFILE)/bera-reth || { echo "ERROR: aarch64 binary not found!"; exit 1; }
	@echo "aarch64 binary verified successfully"

###############################################################################
###                           Tests & Simulation                            ###
###############################################################################

# ask_reset_dir_func checks if the directory passed in exists, and if so asks the user whether it
# should delete it. Note that on linux, docker may have created the directory with root
# permissions, so we may need to ask the user to delete it with sudo
define ask_reset_dir_func
	@abs_path=$(abspath $(1)); \
	if test -d "$$abs_path"; then \
		read -p "Directory '$$abs_path' exists. Do you want to delete it? (y/n): " confirm && \
		if [ "$$confirm" = "y" ]; then \
			echo "Deleting directory '$$abs_path'..."; \
			rm -rf "$$abs_path" 2>/dev/null || sudo rm -rf "$$abs_path"; \
			if test -d "$$abs_path"; then \
				echo "Failed to delete directory '$$abs_path'."; \
				exit 1; \
			fi; \
		fi \
	else \
		echo "Directory '$$abs_path' does not exist."; \
	fi
endef

ETH_DATA_DIR = ${BEACON_KIT}/.tmp/beacond/eth-home
JWT_PATH = ${BEACON_KIT}/testing/files/jwt.hex
IPC_PATH = ${BEACON_KIT}/.tmp/beacond/eth-home/eth-engine.ipc
ETH_GENESIS_PATH = ${BEACON_KIT}/.tmp/beacond/eth-genesis.json

## Start an ephemeral `bera-reth` node using the local `reth` binary (no Docker)
start-bera-reth-local:
	cargo build
	$(call ask_reset_dir_func, $(ETH_DATA_DIR))
	./target/debug/bera-reth node \
		--chain $(ETH_GENESIS_PATH) \
		--http \
		--http.addr "0.0.0.0" \
		--http.port 8545 \
		--http.api eth,net \
		--authrpc.addr "0.0.0.0" \
		--authrpc.jwtsecret $(JWT_PATH) \
		--datadir $(ETH_DATA_DIR) \
		--ipcpath $(IPC_PATH) \
		--engine.persistence-threshold 0 \
		--engine.memory-block-buffer-target 0