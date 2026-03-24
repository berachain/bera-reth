use crate::flashblocks::{
    FlashBlockCompleteSequence, InProgressFlashBlockRx,
    cache::SequenceManager,
    payload::PendingFlashBlock,
    traits::{FlashblockPayload, FlashblockPayloadBase},
    worker::FlashBlockBuilder,
};
use alloy_primitives::B256;
use futures_util::{FutureExt, Stream, StreamExt};
use metrics::{Gauge, Histogram};
use reth_evm::ConfigureEvm;
use reth_metrics::Metrics;
use reth_primitives_traits::{AlloyBlockHeader, BlockTy, HeaderTy, NodePrimitives, ReceiptTy};
use reth_revm::cached::CachedReads;
use reth_storage_api::{BlockReaderIdExt, StateProviderFactory};
use reth_tasks::TaskExecutor;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{broadcast, oneshot, watch},
    time::sleep,
};
use tracing::*;

const CONNECTION_BACKOUT_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct FlashBlockService<N, S, EvmConfig, Provider, P>
where
    N: NodePrimitives,
    P: FlashblockPayload,
    EvmConfig: ConfigureEvm<Primitives = N, NextBlockEnvCtx: From<P::Base> + Unpin>,
{
    incoming_flashblock_rx: S,
    in_progress_tx: watch::Sender<Option<FlashBlockBuildInfo>>,
    received_flashblocks_tx: broadcast::Sender<Arc<P>>,
    builder: FlashBlockBuilder<EvmConfig, Provider, P::Base>,
    spawner: TaskExecutor,
    job: Option<BuildJob<N>>,
    sequences: SequenceManager<P>,
    metrics: FlashBlockServiceMetrics,
}

impl<N, S, EvmConfig, Provider, P> FlashBlockService<N, S, EvmConfig, Provider, P>
where
    N: NodePrimitives,
    P: FlashblockPayload<SignedTx = N::SignedTx>,
    S: Stream<Item = eyre::Result<P>> + Unpin + 'static,
    EvmConfig:
        ConfigureEvm<Primitives = N, NextBlockEnvCtx: From<P::Base> + Unpin> + Clone + 'static,
    Provider: StateProviderFactory
        + BlockReaderIdExt<
            Header = HeaderTy<N>,
            Block = BlockTy<N>,
            Transaction = N::SignedTx,
            Receipt = ReceiptTy<N>,
        > + Unpin
        + Clone
        + 'static,
{
    pub fn new(
        incoming_flashblock_rx: S,
        evm_config: EvmConfig,
        provider: Provider,
        spawner: TaskExecutor,
        compute_state_root: bool,
    ) -> Self {
        let (in_progress_tx, _) = watch::channel(None);
        let (received_flashblocks_tx, _) = broadcast::channel(128);
        Self {
            incoming_flashblock_rx,
            in_progress_tx,
            received_flashblocks_tx,
            builder: FlashBlockBuilder::new(evm_config, provider),
            spawner,
            job: None,
            sequences: SequenceManager::new(compute_state_root),
            metrics: FlashBlockServiceMetrics::default(),
        }
    }

    pub const fn flashblocks_broadcaster(&self) -> &broadcast::Sender<Arc<P>> {
        &self.received_flashblocks_tx
    }

    pub const fn block_sequence_broadcaster(
        &self,
    ) -> &broadcast::Sender<FlashBlockCompleteSequence<P>> {
        self.sequences.block_sequence_broadcaster()
    }

    pub fn subscribe_block_sequence(&self) -> broadcast::Receiver<FlashBlockCompleteSequence<P>> {
        self.sequences.subscribe_block_sequence()
    }

    pub fn subscribe_in_progress(&self) -> InProgressFlashBlockRx {
        self.in_progress_tx.subscribe()
    }

    pub async fn run(mut self, tx: watch::Sender<Option<PendingFlashBlock<N>>>) {
        loop {
            tokio::select! {
                Some(result) = async {
                    match self.job.as_mut() {
                        Some((_, rx)) => rx.await.ok(),
                        None => std::future::pending().await,
                    }
                } => {
                    let (start_time, _) = self.job.take().unwrap();
                    let _ = self.in_progress_tx.send(None);

                    match result {
                        Ok(Some((pending, cached_reads))) => {
                            let parent_hash = pending.parent_hash();
                            self.sequences
                                .on_build_complete(parent_hash, Some((pending.clone(), cached_reads)));

                            let elapsed = start_time.elapsed();
                            self.metrics.execution_duration.record(elapsed.as_secs_f64());

                            let _ = tx.send(Some(pending));
                        }
                        Ok(None) => {
                            trace!(target: "flashblocks", "Build job returned None");
                        }
                        Err(err) => {
                            warn!(target: "flashblocks", %err, "Build job failed");
                        }
                    }
                }

                result = self.incoming_flashblock_rx.next() => {
                    match result {
                        Some(Ok(flashblock)) => {
                            self.process_flashblock(flashblock);

                            while let Some(result) = self.incoming_flashblock_rx.next().now_or_never().flatten() {
                                match result {
                                    Ok(fb) => self.process_flashblock(fb),
                                    Err(err) => warn!(target: "flashblocks", %err, "Error receiving flashblock"),
                                }
                            }

                            self.try_start_build_job();
                        }
                        Some(Err(err)) => {
                            warn!(
                                target: "flashblocks",
                                %err,
                                retry_period = CONNECTION_BACKOUT_PERIOD.as_secs(),
                                "Error receiving flashblock"
                            );
                            sleep(CONNECTION_BACKOUT_PERIOD).await;
                        }
                        None => {
                            warn!(target: "flashblocks", "Flashblock stream ended");
                            break;
                        }
                    }
                }
            }
        }
    }

    fn process_flashblock(&mut self, flashblock: P) {
        self.notify_received_flashblock(&flashblock);

        if flashblock.index() == 0 {
            self.metrics.last_flashblock_length.record(self.sequences.pending().count() as f64);
        }

        if let Err(err) = self.sequences.insert_flashblock(flashblock) {
            warn!(target: "flashblocks", %err, "Failed to insert flashblock");
        }
    }

    fn notify_received_flashblock(&self, flashblock: &P) {
        if self.received_flashblocks_tx.receiver_count() > 0 {
            let _ = self.received_flashblocks_tx.send(Arc::new(flashblock.clone()));
        }
    }

    fn try_start_build_job(&mut self) {
        if self.job.is_some() {
            return;
        }

        let Some(latest) = self.builder.provider().latest_header().ok().flatten() else {
            return;
        };

        let Some(args) = self.sequences.next_buildable_args(latest.hash(), latest.timestamp())
        else {
            return;
        };

        let fb_info = FlashBlockBuildInfo {
            parent_hash: args.base.parent_hash(),
            index: args.last_flashblock_index,
            block_number: args.base.block_number(),
        };
        self.metrics.current_block_height.set(fb_info.block_number as f64);
        self.metrics.current_index.set(fb_info.index as f64);
        let _ = self.in_progress_tx.send(Some(fb_info));

        let (tx, rx) = oneshot::channel();
        let builder = self.builder.clone();
        self.spawner.spawn_blocking(move || {
            let _ = tx.send(builder.execute(args));
        });
        self.job = Some((Instant::now(), rx));
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FlashBlockBuildInfo {
    pub parent_hash: B256,
    pub index: u64,
    pub block_number: u64,
}

type BuildJob<N> =
    (Instant, oneshot::Receiver<eyre::Result<Option<(PendingFlashBlock<N>, CachedReads)>>>);

#[derive(Metrics)]
#[metrics(scope = "flashblock_service")]
struct FlashBlockServiceMetrics {
    /// Number of flashblocks in the last completed sequence.
    last_flashblock_length: Histogram,
    /// Duration of the last flashblock execution in seconds.
    execution_duration: Histogram,
    /// Current block height being processed.
    current_block_height: Gauge,
    /// Current flashblock index within the sequence.
    current_index: Gauge,
}
