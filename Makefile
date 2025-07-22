#!/usr/bin/make -f

###############################################################################
###                               Variables                                 ###
###############################################################################

GIT_SHA ?= $(shell git rev-parse HEAD)
GIT_TAG ?= $(shell git describe --tags --abbrev=0 2>/dev/null || echo "dev")
BIN_DIR = "dist/bin"
CARGO_TARGET_DIR ?= target
PROFILE ?= release
# Features: Optimized features matching upstream Reth
ifeq ($(OS),Windows_NT)
	FEATURES ?= asm-keccak min-debug-logs
else
	FEATURES ?= jemalloc asm-keccak min-debug-logs
endif
DOCKER_IMAGE_NAME ?= bera-reth

###############################################################################
###                               Docker                                    ###
###############################################################################

# Note: Multi-platform builds use buildx with QEMU emulation.
# In CI, this is handled automatically by setup-buildx-action and setup-qemu-action.
# For local development, ensure Docker Desktop has multi-platform support enabled.
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

.PHONY: docker-build-debug
docker-build-debug: ## Fast debug build using Docker multistage (no cross-compilation needed).
	@echo "Building debug Docker image with in-container compilation..."
	docker build --file Dockerfile.debug --tag $(DOCKER_IMAGE_NAME):debug \
		--build-arg COMMIT=$(GIT_SHA) \
		--build-arg VERSION=$(GIT_TAG) \
		.

.PHONY: docker-build-push-nightly
docker-build-push-nightly: ## Build and push cross-arch Docker image tagged with nightly.
	$(call docker_build_push,nightly,nightly)

.PHONY: docker-build-push-nightly-profiling
docker-build-push-nightly-profiling: ## Build and push cross-arch Docker image with profiling profile tagged with nightly-profiling.
	$(call docker_build_push,nightly-profiling,nightly-profiling)

# Create a cross-arch Docker image with the given tags and push it
define docker_build_push
	docker buildx build --file ./Dockerfile . \
		--platform linux/amd64,linux/arm64 \
		--tag $(DOCKER_IMAGE_NAME):$(1) \
		--tag $(DOCKER_IMAGE_NAME):$(2) \
		--build-arg COMMIT=$(GIT_SHA) \
		--build-arg VERSION=$(GIT_TAG) \
		--build-arg BUILD_PROFILE=$(PROFILE) \
		--build-arg FEATURES="$(FEATURES)" \
		--provenance=false \
		--push
endef

# Local build targets for development
.PHONY: build
build: ## Build bera-reth locally
	cargo build --features "$(FEATURES)" --profile "$(PROFILE)"

.PHONY: build-release
build-release: ## Build bera-reth with release profile
	$(MAKE) build PROFILE=release

.PHONY: build-maxperf
build-maxperf: ## Build bera-reth with maxperf profile
	$(MAKE) build PROFILE=maxperf

###############################################################################
###                               Development                               ###
###############################################################################

.PHONY: pr
pr: ## Run all checks that are run in CI for pull requests
	@echo "Running all PR checks..."
	@echo "1. Checking code formatting..."
	cargo +nightly fmt --all -- --check
	@echo "2. Checking TOML formatting..."
	dprint check
	@echo "3. Running clippy..."
	cargo +nightly clippy --all-targets --all-features -- -D warnings
	@echo "4. Running security audit..."
	cargo deny check
	@echo "5. Checking unused dependencies..."
	cargo +nightly udeps --all-features --locked
	@echo "6. Building documentation..."
	RUSTDOCFLAGS="-D warnings" cargo doc --all --no-deps --document-private-items
	@echo "7. Running tests..."
	cargo test --all --locked --verbose
	@echo "All PR checks passed! ✅"

.PHONY: pr-fix
pr-fix: ## Auto-fix formatting issues
	@echo "Auto-fixing formatting issues..."
	cargo +nightly fmt --all
	dprint fmt
	@echo "Formatting fixed! ✅"

###############################################################################
###                           Tests & Simulation                            ###
###############################################################################

# Test coverage configuration
COV_FILE := lcov.info

.PHONY: test
test: ## Run all tests
	cargo test --all --locked --verbose

.PHONY: cov-unit
cov-unit: ## Run unit tests with coverage using cargo-llvm-cov
	rm -f $(COV_FILE)
	cargo llvm-cov --lcov --output-path $(COV_FILE) --all --locked

.PHONY: cov-report-html
cov-report-html: cov-unit ## Generate HTML coverage report and open in browser
	cargo llvm-cov report --html
	@echo "Coverage report generated in target/llvm-cov/html/index.html"

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

## Start an ephemeral `bera-reth` node using the local binary (no Docker)
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