use super::{
    engine::{EvalOutcome, evaluate_line},
    rpc::RpcClient,
};
use eyre::Result;
use serde_json::Value;

pub async fn run_exec(rpc: &RpcClient, script: &str) -> Result<()> {
    let mut last = None;
    match evaluate_line(rpc, script, &mut last).await? {
        EvalOutcome::Value(value) => print_raw_json(&value),
        EvalOutcome::Help => print_help(),
        EvalOutcome::Noop | EvalOutcome::Exit => {}
    }
    Ok(())
}

fn print_raw_json(value: &Value) {
    println!("{}", serde_json::to_string(value).unwrap_or_else(|_| value.to_string()));
}

fn print_help() {
    println!("Usage:");
    println!("  <method> [json_params]   (RPC call; dots become underscores, e.g. eth.blockNumber)");
    println!("  .count | .len | .first | .last | .[0] | .[0].field | .map(.field)");
}
