use crate::{primitives::Block, transaction::BerachainTxEnvelope};
use reth_evm::{
    block::{BlockExecutionError, BlockExecutorFactory},
    eth::EthBlockExecutionCtx,
    execute::{BlockAssembler, BlockAssemblerInput},
};
use reth_primitives_traits::Receipt;

pub struct BerachainAssembler;

impl<F> BlockAssembler<F> for BerachainAssembler
where
    F: for<'a> BlockExecutorFactory<
            ExecutionCtx<'a> = EthBlockExecutionCtx<'a>,
            Transaction = BerachainTxEnvelope,
            Receipt: Receipt,
        >,
{
    type Block = Block;

    fn assemble_block(
        &self,
        input: BlockAssemblerInput<'_, '_, F>,
    ) -> Result<Self::Block, BlockExecutionError> {
        todo!()
    }
}
