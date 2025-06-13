#!/usr/bin/make -f

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
	$(call ask_reset_dir_func, $(ETH_DATA_DIR))
	/Users/rezbera/Code/bera-reth/target/debug/bera-reth node \
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