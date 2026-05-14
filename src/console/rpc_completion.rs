//! Static method-name hints for the embedded `console` REPL.
//!
//! The REPL completes `namespace.method` tokens (e.g. `eth.getLogs`). JSON-RPC uses
//! `namespace_method`; this module only stores the **suffix** after the namespace
//! (the part after `_` or `.`), taken from upstream reth’s `#[method(name = "...")]`
//! macros in `rpc-eth-api` and `rpc-api`, plus Berachain’s `beradmin_*` surface.
//! When upstream adds RPCs, refresh these tables from the same reth sources.
//!
//! Use `RPC_NAMESPACE_TABLE` to walk all namespaces, or `method_suffixes` /
//! `dot_completions_for_namespace` for one namespace.

/// Berachain extension namespace (`beradmin_*` JSON-RPC; matches `#[rpc(namespace = "beradmin")]`).
pub const BERA_ADMIN_METHOD_SUFFIXES: &[&str] = &[
    "detailedPeers",
    "nodeStatus",
    "banPeer",
    "penalizePeer",
    "prepareCanary",
    "submitCanary",
    "exportSealedTxFacts",
];

/// `eth_*` methods from reth.
pub const ETH_METHOD_SUFFIXES: &[&str] = &[
    "accounts",
    "blobBaseFee",
    "blockNumber",
    "call",
    "callBundle",
    "callMany",
    "cancelBundle",
    "cancelPrivateTransaction",
    "chainId",
    "coinbase",
    "config",
    "createAccessList",
    "estimateGas",
    "feeHistory",
    "fillTransaction",
    "gasPrice",
    "getAccount",
    "getAccountInfo",
    "getBalance",
    "getBlockAccessListByBlockHash",
    "getBlockAccessListByBlockNumber",
    "getBlockByHash",
    "getBlockByNumber",
    "getBlockReceipts",
    "getBlockTransactionCountByHash",
    "getBlockTransactionCountByNumber",
    "getCode",
    "getFilterChanges",
    "getFilterLogs",
    "getHeaderByHash",
    "getHeaderByNumber",
    "getLogs",
    "getProof",
    "getRawTransactionByBlockHashAndIndex",
    "getRawTransactionByBlockNumberAndIndex",
    "getRawTransactionByHash",
    "getStorageAt",
    "getTransactionByBlockHashAndIndex",
    "getTransactionByBlockNumberAndIndex",
    "getTransactionByHash",
    "getTransactionBySenderAndNonce",
    "getTransactionCount",
    "getTransactionReceipt",
    "getUncleByBlockHashAndIndex",
    "getUncleByBlockNumberAndIndex",
    "getUncleCountByBlockHash",
    "getUncleCountByBlockNumber",
    "getWork",
    "hashrate",
    "maxPriorityFeePerGas",
    "mining",
    "newBlockFilter",
    "newFilter",
    "newPendingTransactionFilter",
    "protocolVersion",
    "sendBundle",
    "sendPrivateRawTransaction",
    "sendPrivateTransaction",
    "sendRawTransaction",
    "sendRawTransactionConditional",
    "sendRawTransactionSync",
    "sendTransaction",
    "sign",
    "signTransaction",
    "signTypedData",
    "simulateV1",
    "submitHashrate",
    "submitWork",
    "syncing",
    "uninstallFilter",
];

/// `net_*` methods from reth.
pub const NET_METHOD_SUFFIXES: &[&str] = &["listening", "peerCount", "version"];

/// `web3_*` methods from reth.
pub const WEB3_METHOD_SUFFIXES: &[&str] = &["clientVersion", "sha3"];

/// `txpool_*` methods from reth.
pub const TXPOOL_METHOD_SUFFIXES: &[&str] = &["content", "contentFrom", "inspect", "status"];

/// `rpc_*` methods from reth.
pub const RPC_API_METHOD_SUFFIXES: &[&str] = &["modules"];

/// `admin_*` methods from reth.
pub const ADMIN_METHOD_SUFFIXES: &[&str] = &[
    "addPeer",
    "addTrustedPeer",
    "clearTxpool",
    "nodeInfo",
    "peers",
    "removePeer",
    "removeTrustedPeer",
];

/// `trace_*` methods from reth.
pub const TRACE_METHOD_SUFFIXES: &[&str] = &[
    "block",
    "blockOpcodeGas",
    "call",
    "callMany",
    "filter",
    "get",
    "rawTransaction",
    "replayBlockTransactions",
    "replayTransaction",
    "transaction",
    "transactionOpcodeGas",
];

/// `debug_*` methods from reth.
pub const DEBUG_METHOD_SUFFIXES: &[&str] = &[
    "accountRange",
    "backtraceAt",
    "blockProfile",
    "chainConfig",
    "chaindbCompact",
    "chaindbProperty",
    "codeByHash",
    "cpuProfile",
    "dbAncient",
    "dbAncients",
    "dbGet",
    "dumpBlock",
    "executePayload",
    "executionWitness",
    "executionWitnessByBlockHash",
    "freeOSMemory",
    "freezeClient",
    "gcStats",
    "getAccessibleState",
    "getBadBlocks",
    "getBlockAccessList",
    "getModifiedAccountsByHash",
    "getModifiedAccountsByNumber",
    "getRawBlock",
    "getRawHeader",
    "getRawReceipts",
    "getRawTransaction",
    "getRawTransactions",
    "goTrace",
    "intermediateRoots",
    "memStats",
    "mutexProfile",
    "preimage",
    "printBlock",
    "seedHash",
    "setBlockProfileRate",
    "setGCPercent",
    "setHead",
    "setMutexProfileFraction",
    "setTrieFlushInterval",
    "stacks",
    "standardTraceBadBlockToFile",
    "standardTraceBlockToFile",
    "startCPUProfile",
    "startGoTrace",
    "stateRootWithUpdates",
    "stopCPUProfile",
    "stopGoTrace",
    "storageRangeAt",
    "traceBadBlock",
    "traceBlock",
    "traceBlockByHash",
    "traceBlockByNumber",
    "traceCall",
    "traceCallMany",
    "traceChain",
    "traceTransaction",
    "verbosity",
    "vmodule",
    "writeBlockProfile",
    "writeMemProfile",
    "writeMutexProfile",
];

/// `engine_*` methods from reth.
pub const ENGINE_METHOD_SUFFIXES: &[&str] = &[
    "blockNumber",
    "call",
    "chainId",
    "exchangeCapabilities",
    "forkchoiceUpdatedV1",
    "forkchoiceUpdatedV2",
    "forkchoiceUpdatedV3",
    "getBlobsV1",
    "getBlobsV2",
    "getBlobsV3",
    "getBlockByHash",
    "getBlockByNumber",
    "getBlockReceipts",
    "getClientVersionV1",
    "getCode",
    "getLogs",
    "getPayloadBodiesByHashV1",
    "getPayloadBodiesByHashV2",
    "getPayloadBodiesByRangeV1",
    "getPayloadBodiesByRangeV2",
    "getPayloadV1",
    "getPayloadV2",
    "getPayloadV3",
    "getPayloadV4",
    "getPayloadV5",
    "getPayloadV6",
    "getProof",
    "getTransactionReceipt",
    "newPayloadV1",
    "newPayloadV2",
    "newPayloadV3",
    "newPayloadV4",
    "newPayloadV5",
    "sendRawTransaction",
    "syncing",
];

/// `reth_*` methods from reth.
pub const RETH_METHOD_SUFFIXES: &[&str] = &[
    "getBalanceChangesInBlock",
    "subscribeChainNotifications",
    "subscribeFinalizedChainNotifications",
    "subscribePersistedBlock",
];

/// `ots_*` methods from reth.
pub const OTS_METHOD_SUFFIXES: &[&str] = &[
    "getApiLevel",
    "getBlockDetails",
    "getBlockDetailsByHash",
    "getBlockTransactions",
    "getContractCreator",
    "getHeaderByNumber",
    "getInternalOperations",
    "getTransactionBySenderAndNonce",
    "getTransactionError",
    "hasCode",
    "searchTransactionsAfter",
    "searchTransactionsBefore",
    "traceTransaction",
];

/// `miner_*` methods from reth.
pub const MINER_METHOD_SUFFIXES: &[&str] = &["setExtra", "setGasLimit", "setGasPrice"];

/// `mev_*` methods from reth.
pub const MEV_METHOD_SUFFIXES: &[&str] = &["sendBundle", "simBundle"];

/// `testing_*` methods from reth.
pub const TESTING_METHOD_SUFFIXES: &[&str] = &["buildBlockV1"];

/// `flashbots_*` methods from reth.
pub const FLASHBOTS_METHOD_SUFFIXES: &[&str] = &[
    "validateBuilderSubmissionV1",
    "validateBuilderSubmissionV2",
    "validateBuilderSubmissionV3",
    "validateBuilderSubmissionV4",
    "validateBuilderSubmissionV5",
];

/// All built-in namespaces and their method suffix slices.
pub const RPC_NAMESPACE_TABLE: &[(&str, &[&str])] = &[
    ("eth", ETH_METHOD_SUFFIXES),
    ("net", NET_METHOD_SUFFIXES),
    ("web3", WEB3_METHOD_SUFFIXES),
    ("txpool", TXPOOL_METHOD_SUFFIXES),
    ("rpc", RPC_API_METHOD_SUFFIXES),
    ("admin", ADMIN_METHOD_SUFFIXES),
    ("trace", TRACE_METHOD_SUFFIXES),
    ("debug", DEBUG_METHOD_SUFFIXES),
    ("engine", ENGINE_METHOD_SUFFIXES),
    ("reth", RETH_METHOD_SUFFIXES),
    ("ots", OTS_METHOD_SUFFIXES),
    ("miner", MINER_METHOD_SUFFIXES),
    ("mev", MEV_METHOD_SUFFIXES),
    ("testing", TESTING_METHOD_SUFFIXES),
    ("flashbots", FLASHBOTS_METHOD_SUFFIXES),
    ("beradmin", BERA_ADMIN_METHOD_SUFFIXES),
];

/// Returns method suffixes for a namespace, or empty if unknown.
pub fn method_suffixes(namespace: &str) -> &[&str] {
    for (name, suffixes) in RPC_NAMESPACE_TABLE {
        if *name == namespace {
            return suffixes;
        }
    }
    &[]
}

/// `namespace.method` strings for reedline tab completion.
pub fn dot_completions_for_namespace(namespace: &str) -> Vec<String> {
    method_suffixes(namespace).iter().map(|suffix| format!("{namespace}.{suffix}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth_includes_common_methods() {
        assert!(ETH_METHOD_SUFFIXES.contains(&"getLogs"));
        assert!(ETH_METHOD_SUFFIXES.contains(&"blockNumber"));
    }

    #[test]
    fn unknown_namespace_empty() {
        assert!(method_suffixes("not_a_real_ns").is_empty());
    }

    #[test]
    fn table_covers_all_consts() {
        let mut seen = 0usize;
        for (name, suffixes) in RPC_NAMESPACE_TABLE {
            assert!(!suffixes.is_empty(), "namespace {name} has no suffixes");
            seen += 1;
        }
        assert!(seen >= 10);
    }

    #[test]
    fn bera_admin_dot_forms() {
        let v = dot_completions_for_namespace("beradmin");
        assert!(v.iter().any(|s| s == "beradmin.detailedPeers"));
        assert!(v.iter().any(|s| s == "beradmin.exportSealedTxFacts"));
        assert!(
            !v.iter().any(|s| s == "beradmin.sealedBlockAttribution"),
            "completion must drop the removed method"
        );
    }
}
