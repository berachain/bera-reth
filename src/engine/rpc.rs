use crate::{
    engine::{
        BerachainExecutionData, BerachainExecutionPayloadSidecar, validate_proposer_pubkey_prague1,
    },
    hardforks::BerachainHardforks,
    primitives::header::BlsPublicKey,
};
use alloy_eips::{
    eip4844::{BlobAndProofV1, BlobAndProofV2},
    eip7685::RequestsOrHash,
};
use alloy_primitives::{B256, BlockHash, U64};
use alloy_rpc_types::engine::{
    CancunPayloadFields, ClientVersionV1, ExecutionPayloadBodiesV1, ExecutionPayloadInputV2,
    ExecutionPayloadV1, ExecutionPayloadV3, ForkchoiceState, ForkchoiceUpdated, PayloadId,
    PayloadStatus,
};
// Constructor removed since we're using custom construction
use jsonrpsee_core::{RpcResult, server::RpcModule};
use jsonrpsee_proc_macros::rpc;
use jsonrpsee_types;
use reth::{
    api::NodeTypes,
    chainspec::EthereumHardforks,
    payload::PayloadStore,
    providers::{BlockReader, HeaderProvider, StateProviderFactory},
    rpc::api::IntoEngineApiRpcModule,
};
use reth_engine_primitives::{EngineTypes, EngineValidator};
use reth_node_api::{AddOnsContext, FullNodeComponents};
use reth_node_builder::rpc::{EngineApiBuilder, EngineValidatorBuilder};
use reth_node_core::version::{CARGO_PKG_VERSION, CLIENT_CODE, NAME_CLIENT, VERGEN_GIT_SHA};
use reth_payload_primitives::PayloadTypes;
use reth_rpc_engine_api::{
    EngineApi, EngineApiError, EngineCapabilities, INVALID_PAYLOAD_ATTRIBUTES,
};
use reth_transaction_pool::TransactionPool;
use std::sync::Arc;
use tracing::{debug, trace};

/// Builder for basic [`EngineApi`] implementation.
///
/// This provides a basic default implementation for opstack and ethereum engine API via
/// [`EngineTypes`] and uses the general purpose [`EngineApi`] implementation as the builder's
/// output.
#[derive(Debug, Default)]
pub struct BerachainEngineApiBuilder<EV> {
    engine_validator_builder: EV,
}

impl<N, EV> EngineApiBuilder<N> for BerachainEngineApiBuilder<EV>
where
    N: FullNodeComponents<
        Types: NodeTypes<
            ChainSpec: EthereumHardforks + BerachainHardforks,
            Payload: PayloadTypes<ExecutionData = BerachainExecutionData> + EngineTypes,
        >,
    >,
    EV: EngineValidatorBuilder<N>,
{
    type EngineApi = BerachainEngineApi<
        N::Provider,
        <N::Types as NodeTypes>::Payload,
        N::Pool,
        EV::Validator,
        <N::Types as NodeTypes>::ChainSpec,
    >;

    async fn build_engine_api(self, ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::EngineApi> {
        let Self { engine_validator_builder } = self;

        let engine_validator = engine_validator_builder.build(ctx).await?;
        let client = ClientVersionV1 {
            code: CLIENT_CODE,
            name: NAME_CLIENT.to_string(),
            version: CARGO_PKG_VERSION.to_string(),
            commit: VERGEN_GIT_SHA.to_string(),
        };
        let inner = EngineApi::new(
            ctx.node.provider().clone(),
            ctx.config.chain.clone(),
            ctx.beacon_engine_handle.clone(),
            PayloadStore::new(ctx.node.payload_builder_handle().clone()),
            ctx.node.pool().clone(),
            Box::new(ctx.node.task_executor().clone()),
            client,
            EngineCapabilities::default(),
            engine_validator,
            ctx.config.engine.accept_execution_requests_hash,
        );
        Ok(BerachainEngineApi { inner, chain_spec: ctx.config.chain.clone() })
    }
}

#[cfg_attr(not(feature = "client"), rpc(server, namespace = "engine"), server_bounds(Engine::PayloadAttributes: jsonrpsee::core::DeserializeOwned
))]
#[cfg_attr(feature = "client", rpc(server, client, namespace = "engine", client_bounds(Engine::PayloadAttributes: jsonrpsee::core::Serialize + Clone
), server_bounds(Engine::PayloadAttributes: jsonrpsee::core::DeserializeOwned)))]
pub trait BerachainEngineApi<Engine: EngineTypes> {
    /// See also <https://github.com/ethereum/execution-apis/blob/6709c2a795b707202e93c4f2867fa0bf2640a84f/src/engine/paris.md#engine_newpayloadv1>
    /// Caution: This should not accept the `withdrawals` field
    #[method(name = "newPayloadV1")]
    async fn new_payload_v1(&self, payload: ExecutionPayloadV1) -> RpcResult<PayloadStatus>;

    /// See also <https://github.com/ethereum/execution-apis/blob/584905270d8ad665718058060267061ecfd79ca5/src/engine/shanghai.md#engine_newpayloadv2>
    #[method(name = "newPayloadV2")]
    async fn new_payload_v2(&self, payload: ExecutionPayloadInputV2) -> RpcResult<PayloadStatus>;

    /// Post Cancun payload handler
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/cancun.md#engine_newpayloadv3>
    #[method(name = "newPayloadV3")]
    async fn new_payload_v3(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
    ) -> RpcResult<PayloadStatus>;

    /// Post Prague payload handler
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/prague.md#engine_newpayloadv4>
    #[method(name = "newPayloadV4")]
    async fn new_payload_v4(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
        execution_requests: RequestsOrHash,
    ) -> RpcResult<PayloadStatus>;

    /// Post Prague/Electra1 payload handler
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/prague.md#engine_newpayloadv4>
    #[method(name = "newPayloadV4P11")]
    async fn new_payload_v4_p11(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
        execution_requests: RequestsOrHash,
        parent_proposer_pub_key: BlsPublicKey,
    ) -> RpcResult<PayloadStatus>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6709c2a795b707202e93c4f2867fa0bf2640a84f/src/engine/paris.md#engine_forkchoiceupdatedv1>
    ///
    /// Caution: This should not accept the `withdrawals` field in the payload attributes.
    #[method(name = "forkchoiceUpdatedV1")]
    async fn fork_choice_updated_v1(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<Engine::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated>;

    /// Post Shanghai forkchoice update handler
    ///
    /// This is the same as `forkchoiceUpdatedV1`, but expects an additional `withdrawals` field in
    /// the `payloadAttributes`, if payload attributes are provided.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/6709c2a795b707202e93c4f2867fa0bf2640a84f/src/engine/shanghai.md#engine_forkchoiceupdatedv2>
    ///
    /// Caution: This should not accept the `parentBeaconBlockRoot` field in the payload
    /// attributes.
    #[method(name = "forkchoiceUpdatedV2")]
    async fn fork_choice_updated_v2(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<Engine::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated>;

    /// Post Cancun forkchoice update handler
    ///
    /// This is the same as `forkchoiceUpdatedV2`, but expects an additional
    /// `parentBeaconBlockRoot` field in the `payloadAttributes`, if payload attributes
    /// are provided.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/cancun.md#engine_forkchoiceupdatedv3>
    #[method(name = "forkchoiceUpdatedV3")]
    async fn fork_choice_updated_v3(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<Engine::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated>;

    /// Post Prague/Electra1 forkchoice update handler
    ///
    /// Enhanced forkchoice update for Electra1 with additional validation requirements.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/cancun.md#engine_forkchoiceupdatedv3>
    #[method(name = "forkchoiceUpdatedV3P11")]
    async fn fork_choice_updated_v3_p11(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<Engine::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6709c2a795b707202e93c4f2867fa0bf2640a84f/src/engine/paris.md#engine_getpayloadv1>
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call.
    ///
    /// Caution: This should not return the `withdrawals` field
    ///
    /// Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV1")]
    async fn get_payload_v1(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV1>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6709c2a795b707202e93c4f2867fa0bf2640a84f/src/engine/shanghai.md#engine_getpayloadv2>
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call. Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV2")]
    async fn get_payload_v2(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV2>;

    /// Post Cancun payload handler which also returns a blobs bundle.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/cancun.md#engine_getpayloadv3>
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call. Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV3")]
    async fn get_payload_v3(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV3>;

    /// Post Prague payload handler.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/prague.md#engine_getpayloadv4>
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call. Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV4")]
    async fn get_payload_v4(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV4>;

    /// Post Prague/Electra1 payload handler.
    ///
    /// Enhanced payload retrieval for Electra1 with additional validation requirements.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/main/src/engine/prague.md#engine_getpayloadv4>
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call. Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV4P11")]
    async fn get_payload_v4_p11(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV4>;

    /// Post Osaka payload handler.
    ///
    /// See also <https://github.com/ethereum/execution-apis/blob/15399c2e2f16a5f800bf3f285640357e2c245ad9/src/engine/osaka.md#engine_getpayloadv5>.
    ///
    /// Returns the most recent version of the payload that is available in the corresponding
    /// payload build process at the time of receiving this call. Note:
    /// > Provider software MAY stop the corresponding build process after serving this call.
    #[method(name = "getPayloadV5")]
    async fn get_payload_v5(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<Engine::ExecutionPayloadEnvelopeV5>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6452a6b194d7db269bf1dbd087a267251d3cc7f8/src/engine/shanghai.md#engine_getpayloadbodiesbyhashv1>
    #[method(name = "getPayloadBodiesByHashV1")]
    async fn get_payload_bodies_by_hash_v1(
        &self,
        block_hashes: Vec<BlockHash>,
    ) -> RpcResult<ExecutionPayloadBodiesV1>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6452a6b194d7db269bf1dbd087a267251d3cc7f8/src/engine/shanghai.md#engine_getpayloadbodiesbyrangev1>
    ///
    /// Returns the execution payload bodies by the range starting at `start`, containing `count`
    /// blocks.
    ///
    /// WARNING: This method is associated with the `BeaconBlocksByRange` message in the consensus
    /// layer p2p specification, meaning the input should be treated as untrusted or potentially
    /// adversarial.
    ///
    /// Implementers should take care when acting on the input to this method, specifically
    /// ensuring that the range is limited properly, and that the range boundaries are computed
    /// correctly and without panics.
    #[method(name = "getPayloadBodiesByRangeV1")]
    async fn get_payload_bodies_by_range_v1(
        &self,
        start: U64,
        count: U64,
    ) -> RpcResult<ExecutionPayloadBodiesV1>;

    /// This function will return the [`ClientVersionV1`] object.
    /// See also:
    /// <https://github.com/ethereum/execution-apis/blob/03911ffc053b8b806123f1fc237184b0092a485a/src/engine/identification.md#engine_getclientversionv1>
    ///
    ///
    /// - When connected to a single execution client, the consensus client **MUST** receive an
    ///   array with a single `ClientVersionV1` object.
    /// - When connected to multiple execution clients via a multiplexer, the multiplexer **MUST**
    ///   concatenate the responses from each execution client into a single,
    /// flat array before returning the response to the consensus client.
    #[method(name = "getClientVersionV1")]
    async fn get_client_version_v1(
        &self,
        client_version: ClientVersionV1,
    ) -> RpcResult<Vec<ClientVersionV1>>;

    /// See also <https://github.com/ethereum/execution-apis/blob/6452a6b194d7db269bf1dbd087a267251d3cc7f8/src/engine/common.md#capabilities>
    #[method(name = "exchangeCapabilities")]
    async fn exchange_capabilities(&self, capabilities: Vec<String>) -> RpcResult<Vec<String>>;

    /// Fetch blobs for the consensus layer from the blob store.
    #[method(name = "getBlobsV1")]
    async fn get_blobs_v1(
        &self,
        versioned_hashes: Vec<B256>,
    ) -> RpcResult<Vec<Option<BlobAndProofV1>>>;

    /// Fetch blobs for the consensus layer from the blob store.
    ///
    /// Returns a response only if blobs and proofs are present for _all_ of the versioned hashes:
    ///     2. Client software MUST return null in case of any missing or older version blobs.
    #[method(name = "getBlobsV2")]
    async fn get_blobs_v2(
        &self,
        versioned_hashes: Vec<B256>,
    ) -> RpcResult<Option<Vec<BlobAndProofV2>>>;
}

#[derive(Debug)]
pub struct BerachainEngineApi<Provider, PayloadT: PayloadTypes, Pool, Validator, ChainSpec> {
    inner: EngineApi<Provider, PayloadT, Pool, Validator, ChainSpec>,
    chain_spec: Arc<ChainSpec>,
}

#[async_trait::async_trait]
impl<Provider, EngineT, Pool, Validator, ChainSpec> BerachainEngineApiServer<EngineT>
    for BerachainEngineApi<Provider, EngineT, Pool, Validator, ChainSpec>
where
    Provider: HeaderProvider + BlockReader + StateProviderFactory + 'static,
    EngineT: EngineTypes<ExecutionData = BerachainExecutionData>,
    Pool: TransactionPool + 'static,
    Validator: EngineValidator<EngineT>,
    ChainSpec: EthereumHardforks + BerachainHardforks + Send + Sync + 'static,
{
    async fn new_payload_v1(&self, payload: ExecutionPayloadV1) -> RpcResult<PayloadStatus> {
        trace!(target: "rpc::engine", "Serving engine_newPayloadV1");
        let berachain_payload = BerachainExecutionData::from(payload);
        Ok(self.inner.new_payload_v1_metered(berachain_payload).await?)
    }

    async fn new_payload_v2(&self, payload: ExecutionPayloadInputV2) -> RpcResult<PayloadStatus> {
        trace!(target: "rpc::engine", "Serving engine_newPayloadV2");
        let berachain_payload = BerachainExecutionData::from(payload);
        Ok(self.inner.new_payload_v2_metered(berachain_payload).await?)
    }

    async fn new_payload_v3(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
    ) -> RpcResult<PayloadStatus> {
        trace!(target: "rpc::engine", "Serving engine_newPayloadV3");
        let berachain_payload = BerachainExecutionData::new(
            payload.into(),
            BerachainExecutionPayloadSidecar::v3(CancunPayloadFields {
                versioned_hashes,
                parent_beacon_block_root,
            }),
        );
        Ok(self.inner.new_payload_v3_metered(berachain_payload).await?)
    }

    async fn new_payload_v4(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
        execution_requests: RequestsOrHash,
    ) -> RpcResult<PayloadStatus> {
        trace!(target: "rpc::engine", "Serving engine_newPayloadV4");

        // Accept requests as a hash only if it is explicitly allowed
        if execution_requests.is_hash() && !self.inner.accept_execution_requests_hash() {
            return Err(EngineApiError::UnexpectedRequestsHash.into());
        }

        if self.chain_spec.is_prague1_active_at_timestamp(payload.timestamp()) {
            return Err(EngineApiError::other(jsonrpsee_types::ErrorObject::owned(
                INVALID_PAYLOAD_ATTRIBUTES,
                "newPayloadV4P11 required for Prague1 fork, use newPayloadV4P11 instead"
                    .to_string(),
                None::<()>,
            ))
            .into());
        }

        // Validate Electra1 requirements - parent_proposer_pub_key is None
        validate_proposer_pubkey_prague1(&*self.chain_spec, payload.timestamp(), None).map_err(
            |error| {
                EngineApiError::other(jsonrpsee_types::ErrorObject::owned(
                    INVALID_PAYLOAD_ATTRIBUTES,
                    error.to_string(),
                    None::<()>,
                ))
            },
        )?;

        let berachain_payload = BerachainExecutionData::new(
            payload.into(),
            BerachainExecutionPayloadSidecar::v4(
                CancunPayloadFields { versioned_hashes, parent_beacon_block_root },
                execution_requests,
                None,
            ),
        );

        Ok(self.inner.new_payload_v4_metered(berachain_payload).await?)
    }

    async fn new_payload_v4_p11(
        &self,
        payload: ExecutionPayloadV3,
        versioned_hashes: Vec<B256>,
        parent_beacon_block_root: B256,
        execution_requests: RequestsOrHash,
        parent_proposer_pub_key: BlsPublicKey,
    ) -> RpcResult<PayloadStatus> {
        trace!(target: "rpc::engine", "Serving engine_newPayloadV4P11");
        trace!(target: "rpc::engine", "received parent_proposer_pub_key {:?}", parent_proposer_pub_key);

        if !self.chain_spec.is_prague1_active_at_timestamp(payload.timestamp()) {
            return Err(EngineApiError::other(jsonrpsee_types::ErrorObject::owned(
                INVALID_PAYLOAD_ATTRIBUTES,
                "Prague1 fork not active, use newPayloadV4 instead".to_string(),
                None::<()>,
            ))
            .into());
        }

        // Validate Electra1 requirements - parent_proposer_pub_key is required for P11
        validate_proposer_pubkey_prague1(
            &*self.chain_spec,
            payload.timestamp(),
            Some(parent_proposer_pub_key),
        )
        .map_err(|error| {
            EngineApiError::other(jsonrpsee_types::ErrorObject::owned(
                INVALID_PAYLOAD_ATTRIBUTES,
                error.to_string(),
                None::<()>,
            ))
        })?;

        // Accept requests as a hash only if it is explicitly allowed
        if execution_requests.is_hash() && !self.inner.accept_execution_requests_hash() {
            return Err(EngineApiError::UnexpectedRequestsHash.into());
        }

        let berachain_payload = BerachainExecutionData::new(
            payload.into(),
            BerachainExecutionPayloadSidecar::v4(
                CancunPayloadFields { versioned_hashes, parent_beacon_block_root },
                execution_requests,
                Some(parent_proposer_pub_key),
            ),
        );

        Ok(self.inner.new_payload_v4_metered(berachain_payload).await?)
    }

    async fn fork_choice_updated_v1(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<EngineT::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated> {
        trace!(target: "rpc::engine", "Serving engine_forkchoiceUpdatedV1");
        Ok(self.inner.fork_choice_updated_v1_metered(fork_choice_state, payload_attributes).await?)
    }

    async fn fork_choice_updated_v2(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<EngineT::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated> {
        trace!(target: "rpc::engine", "Serving engine_forkchoiceUpdatedV2");
        Ok(self.inner.fork_choice_updated_v2_metered(fork_choice_state, payload_attributes).await?)
    }

    async fn fork_choice_updated_v3(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<EngineT::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated> {
        trace!(target: "rpc::engine", "Serving engine_forkchoiceUpdatedV3");
        Ok(self.inner.fork_choice_updated_v3_metered(fork_choice_state, payload_attributes).await?)
    }

    async fn fork_choice_updated_v3_p11(
        &self,
        fork_choice_state: ForkchoiceState,
        payload_attributes: Option<EngineT::PayloadAttributes>,
    ) -> RpcResult<ForkchoiceUpdated> {
        trace!(target: "rpc::engine", "Serving engine_forkchoiceUpdatedV3P11");
        Ok(self.inner.fork_choice_updated_v3_metered(fork_choice_state, payload_attributes).await?)
    }

    async fn get_payload_v1(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV1> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadV1");
        Ok(self.inner.get_payload_v1_metered(payload_id).await?)
    }

    async fn get_payload_v2(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV2> {
        debug!(target: "rpc::engine", id = %payload_id, "Serving engine_getPayloadV2");
        Ok(self.inner.get_payload_v2_metered(payload_id).await?)
    }

    async fn get_payload_v3(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV3> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadV3");
        Ok(self.inner.get_payload_v3_metered(payload_id).await?)
    }

    async fn get_payload_v4(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV4> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadV4");
        Ok(self.inner.get_payload_v4_metered(payload_id).await?)
    }

    async fn get_payload_v4_p11(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV4> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadV4P11");
        Ok(self.inner.get_payload_v4_metered(payload_id).await?)
    }

    async fn get_payload_v5(
        &self,
        payload_id: PayloadId,
    ) -> RpcResult<EngineT::ExecutionPayloadEnvelopeV5> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadV5");
        Ok(self.inner.get_payload_v5_metered(payload_id).await?)
    }

    async fn get_payload_bodies_by_hash_v1(
        &self,
        block_hashes: Vec<BlockHash>,
    ) -> RpcResult<ExecutionPayloadBodiesV1> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadBodiesByHashV1");
        Ok(self.inner.get_payload_bodies_by_hash_v1_metered(block_hashes).await?)
    }

    async fn get_payload_bodies_by_range_v1(
        &self,
        start: U64,
        count: U64,
    ) -> RpcResult<ExecutionPayloadBodiesV1> {
        trace!(target: "rpc::engine", "Serving engine_getPayloadBodiesByRangeV1");
        Ok(self.inner.get_payload_bodies_by_range_v1_metered(start.to(), count.to()).await?)
    }

    async fn get_client_version_v1(
        &self,
        client_version: ClientVersionV1,
    ) -> RpcResult<Vec<ClientVersionV1>> {
        trace!(target: "rpc::engine", "Serving engine_getClientVersionV1");
        Ok(self.inner.get_client_version_v1(client_version)?)
    }

    async fn exchange_capabilities(&self, _capabilities: Vec<String>) -> RpcResult<Vec<String>> {
        Ok(self.inner.capabilities().list())
    }

    async fn get_blobs_v1(
        &self,
        versioned_hashes: Vec<B256>,
    ) -> RpcResult<Vec<Option<BlobAndProofV1>>> {
        trace!(target: "rpc::engine", "Serving engine_getBlobsV1");
        Ok(self.inner.get_blobs_v1_metered(versioned_hashes)?)
    }

    async fn get_blobs_v2(
        &self,
        versioned_hashes: Vec<B256>,
    ) -> RpcResult<Option<Vec<BlobAndProofV2>>> {
        trace!(target: "rpc::engine", "Serving engine_getBlobsV2");
        Ok(self.inner.get_blobs_v2_metered(versioned_hashes)?)
    }
}

impl<Provider, EngineT, Pool, Validator, ChainSpec> IntoEngineApiRpcModule
    for BerachainEngineApi<Provider, EngineT, Pool, Validator, ChainSpec>
where
    EngineT: EngineTypes,
    Self: BerachainEngineApiServer<EngineT>,
{
    fn into_rpc_module(self) -> RpcModule<()> {
        self.into_rpc().remove_context()
    }
}
